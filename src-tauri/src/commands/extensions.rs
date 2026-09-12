use crate::commands::service::{find_npx_launcher, get_install_dir, NpxLauncher};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdout, Command, Stdio};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{AppHandle, Manager};

/// 在 Windows 下隐藏子进程控制台窗口（避免打开设置/检测依赖时黑窗闪过），非 Windows 为空操作。
#[cfg(target_os = "windows")]
fn hide_console_sync(cmd: &mut Command) {
    cmd.creation_flags(0x08000000);
}
#[cfg(not(target_os = "windows"))]
fn hide_console_sync(_cmd: &mut Command) {}

const PROFILE: &str = "web";
const EXTENSION_EVENT: &str = "extension-log";

#[derive(Debug, Serialize, Clone)]
pub struct ExtensionInfo {
    pub name: String,
    pub version: String,
}

fn emit_log(app: &AppHandle, msg: &str) {
    let _ = app.emit_all(EXTENSION_EVENT, msg.to_string());
}

/// 定位 profile 目录（~/.dsh/profiles/<profile>）
fn profile_dir() -> Result<PathBuf, String> {
    let home = tauri::api::path::home_dir().ok_or_else(|| "无法确定用户主目录".to_string())?;
    Ok(home.join(".dsh").join("profiles").join(PROFILE))
}

/// 计算增强后的 PATH：在当前 PATH 基础上追加 npm 全局 bin 与 git cmd 目录，
/// 仅用于传给子进程（dsh 及其内部 spawnSync 的 pnpm），不修改进程全局环境，
/// 避免 std::env::set_var 在多线程下的 UB 风险。
fn enhanced_path() -> String {
    let mut path = std::env::var("PATH").unwrap_or_default();
    // 追加 npm 全局 bin（pnpm 经 npm install -g 后通常在此目录，确保 dsh 内部能找到 pnpm）
    let mut npm_bin_cmd = Command::new("npm");
    npm_bin_cmd.args(["bin", "-g"]);
    hide_console_sync(&mut npm_bin_cmd);
    if let Ok(out) = npm_bin_cmd.output() {
        if out.status.success() {
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !dir.is_empty() && !path_contains(&path, &dir) {
                path = format!("{};{}", path, dir);
            }
        }
    }
    // 追加 git cmd 目录（winget 默认装到 C:\Program Files\Git\cmd）
    let git_dir = r"C:\Program Files\Git\cmd";
    if Path::new(git_dir).exists() && !path_contains(&path, git_dir) {
        path = format!("{};{}", path, git_dir);
    }
    path
}

fn path_contains(path: &str, dir: &str) -> bool {
    path.split(';')
        .chain(path.split(':'))
        .any(|p| p.eq_ignore_ascii_case(dir))
}

/// 确保 pnpm 可用：已检测到则跳过；否则用 npm 全局安装（绝不升级/重装已有版本）
async fn ensure_pnpm(app: &AppHandle) -> Result<(), String> {
    let mut pnpm_ver = Command::new("pnpm");
    pnpm_ver.arg("--version");
    hide_console_sync(&mut pnpm_ver);
    if let Ok(out) = pnpm_ver.output() {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            emit_log(app, &format!("[依赖] 已检测到 pnpm {}", v));
            return Ok(());
        }
    }

    emit_log(app, "[依赖] 未检测到 pnpm，正在自动安装（npm install -g pnpm）...");
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args([
            "/c", "npm", "install", "-g", "pnpm", "--verbose", "--no-audit", "--no-fund",
        ]);
        c
    } else {
        let mut c = Command::new("npm");
        c.args(["install", "-g", "pnpm", "--verbose", "--no-audit", "--no-fund"]);
        c
    };
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 pnpm 安装失败: {}", e))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    start_ext_readers(app, stdout, stderr);
    let status = tokio::task::spawn_blocking(move || child.wait())
        .await
        .map_err(|e| format!("等待 pnpm 安装失败: {}", e))?
        .map_err(|e| format!("等待 pnpm 安装失败: {}", e))?;
    if !status.success() {
        return Err(
            "pnpm 自动安装失败（可能缺权限或无网络）。请手动执行：npm install -g pnpm".to_string(),
        );
    }

    emit_log(app, "[依赖] pnpm 安装完成");
    Ok(())
}

/// 确保 git 可用：已检测到则跳过；否则用 winget 安装 Git for Windows
async fn ensure_git(app: &AppHandle) -> Result<(), String> {
    let mut git_ver = Command::new("git");
    git_ver.arg("--version");
    hide_console_sync(&mut git_ver);
    if let Ok(out) = git_ver.output() {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            emit_log(app, &format!("[依赖] 已检测到 git {}", v));
            return Ok(());
        }
    }

    emit_log(app, "[依赖] 未检测到 git，正在通过 winget 安装 Git for Windows...");
    let mut winget_cmd = Command::new("winget");
    winget_cmd.args([
        "install",
        "Git.Git",
        "--silent",
        "--accept-package-agreements",
        "--accept-source-agreements",
    ]);
    hide_console_sync(&mut winget_cmd);
    let status = winget_cmd.status();
    match status {
        Ok(s) if s.success() => {
            emit_log(
                app,
                "[依赖] git 安装完成（如后续命令仍找不到 git，请重启本程序以刷新 PATH）",
            );
            Ok(())
        }
        Ok(_) => Err("git 自动安装失败，请手动安装 Git for Windows 后重试".to_string()),
        Err(e) => Err(format!(
            "无法启动 winget 安装 git: {}（请手动安装 Git for Windows）",
            e
        )),
    }
}

/// 用 node + npx-cli.js 启动 dsh 并回流日志（与 service.rs 相同的无黑窗方案）
async fn spawn_dsh(app: &AppHandle, args: &[String]) -> Result<(), String> {
    let launcher =
        find_npx_launcher().ok_or_else(|| "未找到 npx，请先安装 Node.js".to_string())?;
    let mut cmd = match launcher {
        NpxLauncher::NodeCli(node, cli) => {
            let mut c = Command::new(node);
            c.arg(cli);
            c
        }
        NpxLauncher::Direct(npx) => Command::new(npx),
    };
    cmd.args(args)
        .current_dir(get_install_dir())
        .env("PATH", enhanced_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 dsh 失败: {}", e))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    start_ext_readers(app, stdout, stderr);
    let status = tokio::task::spawn_blocking(move || child.wait())
        .await
        .map_err(|e| format!("等待 dsh 退出失败: {}", e))?
        .map_err(|e| format!("等待 dsh 退出失败: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("dsh 命令失败（退出码: {}）", status))
    }
}

fn start_ext_readers(
    app: &AppHandle,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
) {
    if let Some(out) = stdout {
        let h = app.clone();
        tokio::task::spawn_blocking(move || read_ext_pipe(out, h, "stdout"));
    }
    if let Some(err) = stderr {
        let h = app.clone();
        tokio::task::spawn_blocking(move || read_ext_pipe(err, h, "stderr"));
    }
}

fn read_ext_pipe<R: Read>(pipe: R, app: AppHandle, stream: &str) {
    let reader = BufReader::new(pipe);
    for line in reader.lines() {
        match line {
            Ok(l) => {
                let _ = app.emit_all(EXTENSION_EVENT, format!("[{}] {}", stream, l));
            }
            Err(_) => break,
        }
    }
}

// ───────── 命令 ─────────

/// 检查并安装依赖（pnpm / git），供 UI 手动触发
#[tauri::command]
pub async fn ensure_deps(app: AppHandle) -> Result<(), String> {
    ensure_pnpm(&app).await?;
    ensure_git(&app).await?;
    emit_log(&app, "[依赖] pnpm 与 git 已就绪");
    Ok(())
}

/// 通过包名 / URL 添加扩展（dsh plugin --profile web add <source>）
#[tauri::command]
pub async fn install_extension(app: AppHandle, source: String) -> Result<(), String> {
    let source = source.trim().to_string();
    if source.is_empty() {
        return Err("扩展来源不能为空".to_string());
    }

    let is_git = source.starts_with("github:")
        || source.starts_with("git+")
        || source.ends_with(".git");
    if is_git {
        emit_log(
            &app,
            "[提示] 检测到 git 源，pnpm 可能拦截 prepare 构建脚本。若安装卡住，请按 dsh 提示在 pnpm-workspace.yaml 的 allowBuilds 中加入该源后重试。",
        );
    }

    emit_log(&app, &format!("[安装] dsh plugin --profile {} add {}", PROFILE, source));
    ensure_pnpm(&app).await?;
    ensure_git(&app).await?;

    let args = vec![
        "--yes".to_string(),
        "@deepseek-ai/dsh@latest".to_string(),
        "plugin".to_string(),
        "--profile".to_string(),
        PROFILE.to_string(),
        "add".to_string(),
        source.clone(),
        "--loglevel=debug".to_string(),
    ];
    match spawn_dsh(&app, &args).await {
        Ok(()) => {
            emit_log(
                &app,
                &format!("[完成] 扩展 '{}' 已添加，重启 dsh 服务后生效", source),
            );
            Ok(())
        }
        Err(e) => {
            emit_log(&app, &format!("[错误] 安装失败: {}", e));
            Err(e)
        }
    }
}

/// 列出已安装扩展：直接读取 profile 的 package.json dependencies。
/// `dsh plugin add` 会把扩展写进这里；dsh 内置 bundle 在 `dsh.profile.bundles`，
/// 不在此列，因此不会污染用户扩展列表。
/// （注意：不能用 `dsh plugin list --json`，其返回的是 workspace 顶层 importer 信息，
///  不含已安装扩展，实测为 `[{name:"dsh-profile-xxx", path:..., private:true}]`。）
#[tauri::command]
pub async fn list_extensions(app: AppHandle) -> Result<Vec<ExtensionInfo>, String> {
    let dir = profile_dir()?;
    let pkg = dir.join("package.json");
    if !pkg.exists() {
        emit_log(
            &app,
            &format!(
                "[信息] profile '{}' 尚未初始化（请先启动一次 dsh 服务），暂无已安装扩展",
                PROFILE
            ),
        );
        return Ok(vec![]);
    }
    let content =
        fs::read_to_string(&pkg).map_err(|e| format!("读取 profile 配置失败: {}", e))?;
    let v: Value =
        serde_json::from_str(&content).map_err(|e| format!("解析 profile 配置失败: {}", e))?;
    let mut result = Vec::new();
    if let Some(deps) = v.get("dependencies").and_then(|d| d.as_object()) {
        for (name, info) in deps {
            let version = info
                .get("version")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            result.push(ExtensionInfo {
                name: name.clone(),
                version,
            });
        }
    }
    emit_log(&app, &format!("[信息] 已加载 {} 个已安装扩展", result.len()));
    Ok(result)
}

/// 卸载扩展（dsh plugin --profile web remove <pkg>）
#[tauri::command]
pub async fn uninstall_extension(app: AppHandle, pkg: String) -> Result<(), String> {
    let pkg = pkg.trim().to_string();
    if pkg.is_empty() {
        return Err("包名不能为空".to_string());
    }
    emit_log(&app, &format!("[卸载] dsh plugin --profile {} remove {}", PROFILE, pkg));
    ensure_pnpm(&app).await?;
    let args = vec![
        "--yes".to_string(),
        "@deepseek-ai/dsh@latest".to_string(),
        "plugin".to_string(),
        "--profile".to_string(),
        PROFILE.to_string(),
        "remove".to_string(),
        pkg.clone(),
    ];
    match spawn_dsh(&app, &args).await {
        Ok(()) => {
            emit_log(&app, &format!("[完成] 扩展 '{}' 已卸载", pkg));
            Ok(())
        }
        Err(e) => {
            emit_log(&app, &format!("[错误] 卸载失败: {}", e));
            Err(e)
        }
    }
}
