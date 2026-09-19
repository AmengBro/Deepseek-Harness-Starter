use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Manager};
use tokio::process::Command as AsyncCommand;
use tokio::time::Duration;

use crate::commands::service::{get_install_dir, new_dsh_command};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DshVersionInfo {
    /// 当前本机（启动时会用到的）dsh 版本
    pub current: String,
    /// npm registry 上的 latest 版本
    pub latest: String,
    /// 是否需要更新
    pub needs_update: bool,
    /// 版本探测来源："global" | "unknown"
    pub source: String,
}

const NPM_PKG: &str = "@deepseek-ai/dsh";

/// 查询 npm registry 上 @deepseek-ai/dsh 的 latest 版本号
async fn fetch_latest_version() -> Result<String, String> {
    let url = "https://registry.npmjs.org/@deepseek-ai/dsh/latest";
    let client = reqwest::Client::builder()
        .user_agent("DeepseekHarness")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(10))
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

/// 探测当前（启动时会用到的）全局 dsh 版本，不通过 npx 隐式下载。
async fn detect_current_version() -> (String, String) {
    let Some(mut command) = new_dsh_command() else {
        return ("unknown".to_string(), "unknown".to_string());
    };
    command.arg("--version");
    let probe_global = AsyncCommand::from(command).output();

    if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(8), probe_global).await {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return (v, "global".to_string());
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
        "--force",
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
        "--force",
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

/* ==================== npm 镜像自动切换（面向国内小白用户） ==================== */

/// npm 官方源
const NPM_OFFICIAL: &str = "https://registry.npmjs.org";
/// 国内镜像源（淘宝 npmmirror，实测比官方快 6-7 倍）
const NPM_MIRROR: &str = "https://registry.npmmirror.com";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NpmRegistryInfo {
    /// 当前生效的 npm 源
    pub registry: String,
    /// 是否判定为国内网络（连不通境外站点）
    pub in_china: bool,
    /// 当前是否为官方源
    pub is_official: bool,
    /// 本次调用是否发生了自动切换
    pub switched: bool,
    /// 读取/写入失败时的错误信息
    pub error: Option<String>,
}

/// Windows 下隐藏子进程控制台窗口（npm 是 node 脚本，直接 spawn 会闪黑窗）；其他平台空操作
#[cfg(target_os = "windows")]
fn hide_console_cmd(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x08000000);
}
#[cfg(not(target_os = "windows"))]
fn hide_console_cmd(_cmd: &mut Command) {}

/// 读取当前 npm 源（等价 `npm config get registry`）
fn read_npm_registry() -> Result<String, String> {
    let mut cmd = Command::new("npm");
    cmd.args(["config", "get", "registry"]);
    hide_console_cmd(&mut cmd);
    match cmd.output() {
        Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()),
        Err(e) => Err(format!("执行 npm config get registry 失败: {}", e)),
    }
}

/// 写入 npm 源（等价 `npm config set registry <url>`，写入用户级 ~/.npmrc）
fn write_npm_registry(url: &str) -> Result<(), String> {
    let mut cmd = Command::new("npm");
    cmd.args(["config", "set", "registry", url]);
    hide_console_cmd(&mut cmd);
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!(
            "npm config set 失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!("执行 npm config set 失败: {}", e)),
    }
}

/// 判定是否处于国内网络：尝试连接境外站点 chatgpt.com。
/// - 连不通（超时/失败）→ 判定为国内，需要镜像
/// - 能连通 → 说明出口在境外或有全局代理，npm 官方源同样可用，无需切换
/// 用 HTTP 请求而非 ICMP ping：ping 常被防火墙丢弃，HTTP 判定更贴近真实下载场景。
async fn detect_in_china() -> bool {
    // 3 秒足够判定：境外通常 1 秒内响应，国内则连接超时。
    // 这个检测在启动链路里执行，超时设太长会明显拖慢启动。
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return true, // 建不出客户端，保守按国内处理
    };
    client.get("https://chatgpt.com").send().await.is_err()
}

/// 查询当前 npm 源信息（不改任何配置，纯读取）
#[tauri::command]
pub async fn get_npm_registry() -> Result<NpmRegistryInfo, String> {
    let registry = read_npm_registry()?;
    let in_china = detect_in_china().await;
    Ok(NpmRegistryInfo {
        is_official: registry.contains("registry.npmjs.org"),
        registry,
        in_china,
        switched: false,
        error: None,
    })
}

/// 自动保障 npm 源可用：**官方源 + 国内网络** 时自动切到国内镜像。
///
/// 设计考量：
/// - 只在这两个条件同时满足时才动配置，已配镜像或身在境外的用户**不受任何影响**
/// - 写入的是用户级 `~/.npmrc`，对小白是净收益（后续所有 npm/pnpm 操作都加速）
/// - 切换结果通过 `service-log` 事件回传前端，用户可见
#[tauri::command]
pub async fn ensure_npm_mirror(app_handle: AppHandle) -> Result<NpmRegistryInfo, String> {
    let current = read_npm_registry()?;
    let in_china = detect_in_china().await;
    let is_official = current.contains("registry.npmjs.org");

    let mut registry = current;
    let mut switched = false;
    let mut error: Option<String> = None;

    if in_china && is_official {
        match write_npm_registry(NPM_MIRROR) {
            Ok(_) => {
                switched = true;
                registry = NPM_MIRROR.to_string();
                let msg = format!(
                    "[系统] 检测到国内网络且 npm 使用官方源，已自动切换到国内镜像: {}",
                    NPM_MIRROR
                );
                let _ = app_handle.emit_all("service-log", msg);
            }
            Err(e) => {
                error = Some(e.clone());
                let msg = format!("[警告] 自动切换 npm 镜像失败，可手动执行：npm config set registry {}", NPM_MIRROR);
                let _ = app_handle.emit_all("service-log", msg);
            }
        }
    }

    Ok(NpmRegistryInfo {
        is_official: registry.contains("registry.npmjs.org"),
        registry,
        in_china,
        switched,
        error,
    })
}

/// 手动设置 npm 源（供设置页下拉切换：官方源 / 国内镜像）
#[tauri::command]
pub async fn set_npm_registry(
    app_handle: AppHandle,
    registry: String,
) -> Result<NpmRegistryInfo, String> {
    // 仅允许在两个已知源之间切换，避免注入 arbitrary URL
    let target = if registry.contains("npmjs.org") {
        NPM_OFFICIAL
    } else if registry.contains("npmmirror") {
        NPM_MIRROR
    } else {
        return Err("不支持的 npm 源，仅允许官方源或 npmmirror".to_string());
    };

    write_npm_registry(target)?;
    let msg = format!("[系统] npm 源已切换为: {}", target);
    let _ = app_handle.emit_all("service-log", msg);

    let in_china = detect_in_china().await;
    Ok(NpmRegistryInfo {
        is_official: target == NPM_OFFICIAL,
        registry: target.to_string(),
        in_china,
        switched: true,
        error: None,
    })
}
