use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Manager};
use tokio::process::Command as AsyncCommand;
use tokio::time::Duration;

use crate::commands::service::get_install_dir;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DshVersionInfo {
    /// 当前本机（启动时会用到的）dsh 版本
    pub current: String,
    /// npm registry 上的 latest 版本
    pub latest: String,
    /// 是否需要更新
    pub needs_update: bool,
    /// 版本探测来源："global" | "npx" | "unknown"
    pub source: String,
}

const NPM_PKG: &str = "@deepseek-ai/dsh";

/// 查询 npm registry 上 @deepseek-ai/dsh 的 latest 版本号
async fn fetch_latest_version() -> Result<String, String> {
    let url = "https://registry.npmjs.org/@deepseek-ai/dsh/latest";
    let client = reqwest::Client::builder()
        .user_agent("DeepseekHarness")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求 npm registry 失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("npm registry 返回错误: {}", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 registry 响应失败: {}", e))?;
    body["version"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "无法获取 latest 版本号".to_string())
}

/// 探测当前（启动时会用到的）dsh 版本：优先全局 dsh --version，失败回退 npx（与启动命令一致）
async fn detect_current_version() -> (String, String) {
    // 策略1：全局已安装的 dsh
    #[cfg(target_os = "windows")]
    let probe_global = AsyncCommand::new("cmd").args(["/c", "dsh", "--version"]).output();
    #[cfg(not(target_os = "windows"))]
    let probe_global = AsyncCommand::new("dsh").arg("--version").output();

    if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(8), probe_global).await {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return (v, "global".to_string());
            }
        }
    }

    // 策略2：npx（与启动器实际启动命令一致，但首次可能触发下载）
    #[cfg(target_os = "windows")]
    let probe_npx =
        AsyncCommand::new("cmd").args(["/c", "npx", "--yes", NPM_PKG, "--version"]).output();
    #[cfg(not(target_os = "windows"))]
    let probe_npx = AsyncCommand::new("npx")
        .args(["--yes", NPM_PKG, "--version"])
        .output();

    if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(40), probe_npx).await {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return (v, "npx".to_string());
            }
        }
    }

    ("unknown".to_string(), "unknown".to_string())
}

fn parse_version(v: &str) -> Vec<u32> {
    v.trim_start_matches('v')
        .split(|c: char| c == '.' || c == '-' || c == '+')
        .filter_map(|p| p.parse::<u32>().ok())
        .collect()
}

/// 判断 a 是否严格小于 b（用于「当前 < 最新」即需更新）
fn version_less(a: &str, b: &str) -> bool {
    let va = parse_version(a);
    let vb = parse_version(b);
    for (x, y) in va.iter().zip(vb.iter()) {
        if x < y {
            return true;
        }
        if x > y {
            return false;
        }
    }
    va.len() < vb.len()
}

/// 统一回流日志：写入 config/npmlog/dsh-update-YYYY-MM-DD.log 并通过 dsh-update-log 事件推给前端
fn push_log(app_handle: &AppHandle, line: &str) {
    let install_dir = get_install_dir();
    let log_dir = install_dir.join("config").join("npmlog");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join(format!(
        "dsh-update-{}.log",
        chrono::Local::now().format("%Y-%m-%d")
    ));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write;
        let _ = writeln!(
            f,
            "[{}] {}",
            chrono::Local::now().format("%H:%M:%S%.3f"),
            line
        );
    }
    let _ = app_handle.emit_all("dsh-update-log", line.to_string());
}

fn read_pipe<R: Read>(pipe: R, app_handle: AppHandle) {
    let reader = BufReader::new(pipe);
    for line in reader.lines() {
        match line {
            Ok(l) => push_log(&app_handle, &l),
            Err(_) => break,
        }
    }
}

/// 检查 dsh 是否有更新可用：对比当前版本与 npm latest
#[tauri::command]
pub async fn check_dsh_version() -> Result<DshVersionInfo, String> {
    let (current, source) = detect_current_version().await;
    let latest = match fetch_latest_version().await {
        Ok(v) => v,
        Err(_) => {
            // 拿不到 latest 时不阻断，仅返回当前探测结果
            return Ok(DshVersionInfo {
                current: current.clone(),
                latest: "unknown".to_string(),
                needs_update: false,
                source,
            });
        }
    };
    let needs_update = current != "unknown" && version_less(&current, &latest);
    Ok(DshVersionInfo {
        current,
        latest,
        needs_update,
        source,
    })
}

/// 将 dsh 更新到 npm 上的最新版（@latest），实时回流安装日志
#[tauri::command]
pub async fn update_dsh(app_handle: AppHandle) -> Result<String, String> {
    let install_dir = get_install_dir();
    push_log(
        &app_handle,
        "[系统] 开始将 dsh 更新到最新版 (@latest)...",
    );

    #[cfg(target_os = "windows")]
    let mut cmd = Command::new("cmd");
    #[cfg(target_os = "windows")]
    cmd.args([
        "/c",
        "npm",
        "install",
        "-g",
        &format!("{}@latest", NPM_PKG),
        "--verbose",
        "--no-audit",
        "--no-fund",
    ]);

    #[cfg(not(target_os = "windows"))]
    let mut cmd = Command::new("npm");
    #[cfg(not(target_os = "windows"))]
    cmd.args([
        "install",
        "-g",
        &format!("{}@latest", NPM_PKG),
        "--verbose",
        "--no-audit",
        "--no-fund",
    ]);

    cmd.current_dir(&install_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 dsh 更新失败: {}", e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    if let Some(o) = stdout {
        let h = app_handle.clone();
        tokio::task::spawn_blocking(move || read_pipe(o, h));
    }
    if let Some(e) = stderr {
        let h = app_handle.clone();
        tokio::task::spawn_blocking(move || read_pipe(e, h));
    }

    let wait_result = tokio::task::spawn_blocking(move || child.wait()).await;
    let status = match wait_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            push_log(&app_handle, &format!("[错误] 等待更新进程失败: {}", e));
            return Err(format!("等待更新进程失败: {}", e));
        }
        Err(e) => {
            push_log(&app_handle, &format!("[错误] 更新任务异常: {}", e));
            return Err(format!("更新任务异常: {}", e));
        }
    };

    if !status.success() {
        push_log(
            &app_handle,
            "[错误] dsh 更新失败（若装在系统目录，请尝试以管理员身份运行）",
        );
        return Err("dsh 更新失败".to_string());
    }

    // 更新完成后再次探测，确认新版已生效
    let (new_ver, _) = detect_current_version().await;
    push_log(
        &app_handle,
        &format!("[系统] dsh 更新完成，当前版本: {}", new_ver),
    );
    Ok(new_ver)
}
