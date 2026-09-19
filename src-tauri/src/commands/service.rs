use crate::commands::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::process::Command as AsyncCommand;
use tokio::sync::Mutex;

/// 在 Windows 下隐藏子进程的控制台窗口（避免 GUI 程序 spawn cmd/node 时黑窗闪过）。
/// 非 Windows 平台为空操作。用于所有以 .output()/.status() 方式的探测命令。
#[cfg(target_os = "windows")]
fn hide_console(cmd: &mut AsyncCommand) {
    cmd.creation_flags(0x08000000);
}
#[cfg(not(target_os = "windows"))]
fn hide_console(_cmd: &mut AsyncCommand) {}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceStatus {
    pub running: bool,
    pub port: u16,
    pub url: String,
}

pub struct ServiceManager {
    process: Arc<Mutex<Option<Child>>>,
    status: Arc<Mutex<ServiceStatus>>,
    install_dir: PathBuf,
    restart_count: Arc<Mutex<u32>>,
    is_restarting: Arc<AtomicBool>,
    monitor_running: Arc<AtomicBool>,
    /// dsh web 启动后打印的带 token 认证 URL。
    /// 用 `std::sync::Mutex` 以便 `read_pipe` 的同步线程能直接写入，无需跨 await 拿 tokio Mutex。
    /// 前端 iframe 必须用此完整 URL（含 ?token=）访问，否则 dsh 返回 "authentication required"。
    auth_url: Arc<std::sync::Mutex<Option<String>>>,
    /// 防止 `service-ready` 事件重复发送：read_pipe 抓到认证 URL与探测循环就绪
    /// 任一先触发即可，另一方见到 true 即跳过。
    ready_emitted: Arc<AtomicBool>,
}

/// dsh 启动完成后，"主窗口隐藏到托盘"动作。
///
/// 用户设计意图：主窗口只用于启动日志展示，dsh 一起好，主窗口就 hide 让位
/// 给独立 dsh web 窗口（这是 Tauri 1.x 的硬限制——嵌入式 webview 不可用）。
/// 用户可以：
/// - 从系统托盘左键点击 → toggle 主窗口显隐（lib.rs `SystemTrayEvent::LeftClick`）
/// - 或托盘菜单"打开主界面" → show 主窗口（lib.rs "open" 菜单项）
///
/// 该函数集中处理 hide 调用，所有 emit "service-ready" 之后都应调一次。
fn auto_hide_main_window(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_window("main") {
        if window.is_visible().unwrap_or(false) {
            if window.hide().is_ok() {
                let _ = app_handle.emit_all(
                    "service-log",
                    "[系统] dsh 已就绪，主窗口已隐藏到托盘（点击托盘图标可重新打开）"
                        .to_string(),
                );
            }
        }
    }
}

impl ServiceManager {
    pub fn new(install_dir: PathBuf) -> Self {
        Self {
            process: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(ServiceStatus {
                running: false,
                port: 3080,
                url: String::new(),
            })),
            install_dir,
            restart_count: Arc::new(Mutex::new(0)),
            is_restarting: Arc::new(AtomicBool::new(false)),
            monitor_running: Arc::new(AtomicBool::new(false)),
            auth_url: Arc::new(std::sync::Mutex::new(None)),
            ready_emitted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn reset_restart_count(&self) {
        let mut count = self.restart_count.lock().await;
        *count = 0;
    }

    /// 清空上次启动抓到的 auth_url + 重置 ready_emitted CAS 标志位。
    /// 服务重启（手动 stop+start / monitor 自动重启）时**必须调用**，
    /// 否则新 dsh 进程的 token URL 被旧 CAS 跳过 emit，前端永远收不到第二次 service-ready
    /// → 用了不匹配的新旧 URL → dsh 显示 "authentication required" 拒绝访问。
    pub async fn reset_ready_state(&self) {
        if let Ok(mut g) = self.auth_url.lock() {
            *g = None;
        }
        self.ready_emitted.store(false, Ordering::SeqCst);
    }

    /// 只读取当前已就绪的 dsh 认证 URL（含 token），**同步、不阻塞**。
    /// 供系统托盘「打开主界面」直接打开/聚焦 dsh 窗口使用：
    /// - Some(url)：dsh 已就绪 → 托盘点击直达 dsh 界面（不再显示启动日志窗口）
    /// - None：服务还没起来 → 退回显示主窗口（可看日志 / 手动启动）
    pub fn get_auth_url(&self) -> Option<String> {
        self.auth_url.lock().ok().and_then(|g| g.clone())
    }

    /// 同步杀掉服务进程（退出时调用）
    /// 关键优化：仅异步发起 taskkill /F /T 即返回，**绝不阻塞等待子进程退出**——
    /// 之前最多等 2 秒的 try_wait 轮询正是「退出太慢」的元凶。
    /// taskkill 作为独立 OS 进程会在后台把整个 dsh 进程树（cmd → npx → node → ...）清掉，
    /// 应用进程可立即 app.exit，无需陪跑。
    pub fn kill_service_blocking(&self) {
        self.is_restarting.store(true, Ordering::SeqCst);
        let mut process = self.process.blocking_lock();
        if let Some(child) = process.take() {
            let pid = child.id();
            // 先释放 Child 句柄，避免持有导致后续清理语义混乱
            drop(child);
            #[cfg(target_os = "windows")]
            {
                // spawn 异步发起 taskkill，不 wait status——后台静默清理进程树
                let _ = std::process::Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(0x08000000)
                    .spawn();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
        // 立即返回，不 wait——让 app.exit 瞬发，不再 hang 在 av/ntdll 锁
    }
}

pub fn get_install_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn get_npm_cache_dir(install_dir: &PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("DeepseekHarness")
            .join("npm-cache");
    }

    install_dir.join("config").join("npm-cache")
}

pub async fn check_node() -> Result<String, String> {
    // 策略1：直接调用 node（PATH 中有 node 时生效）
    if let Ok(Ok(output)) = tokio::time::timeout(
        Duration::from_secs(3),
        {
            let mut c = AsyncCommand::new("node");
            c.arg("--version");
            hide_console(&mut c);
            c.output()
        },
    ).await {
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }

    // 策略2：通过 cmd /c 调用（Windows cmd 会用完整 PATH 解析）
    #[cfg(target_os = "windows")]
    {
        if let Ok(Ok(output)) = tokio::time::timeout(
            Duration::from_secs(3),
            {
                let mut c = AsyncCommand::new("cmd");
                c.args(["/c", "node", "--version"]);
                hide_console(&mut c);
                c.output()
            },
        ).await {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
        }

        // 策略3：用 where 命令查找 node 路径（std 同步调用，单独加隐藏标志）
        if let Ok(Ok(where_output)) = tokio::time::timeout(
            Duration::from_secs(3),
            {
                let mut c = AsyncCommand::new("where");
                c.arg("node");
                hide_console(&mut c);
                c.output()
            },
        ).await {
            if where_output.status.success() {
                let paths = String::from_utf8_lossy(&where_output.stdout);
                if let Some(first_line) = paths.lines().next() {
                    let node_path = first_line.trim();
                    if let Ok(Ok(output)) = tokio::time::timeout(
                        Duration::from_secs(3),
                        {
                            let mut c = AsyncCommand::new(node_path);
                            c.arg("--version");
                            hide_console(&mut c);
                            c.output()
                        },
                    ).await {
                        if output.status.success() {
                            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
                        }
                    }
                }
            }
        }

        // 策略4：常见安装路径
        let common_paths = [
            r"C:\Program Files\nodejs\node.exe",
            r"C:\Program Files (x86)\nodejs\node.exe",
        ];
        for path in &common_paths {
            if let Ok(Ok(output)) = tokio::time::timeout(
                Duration::from_secs(3),
                {
                    let mut c = AsyncCommand::new(path);
                    c.arg("--version");
                    hide_console(&mut c);
                    c.output()
                },
            ).await {
                if output.status.success() {
                    return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
                }
            }
        }
    }

    // 策略5：非 Windows 平台通过 shell 调用
    #[cfg(not(target_os = "windows"))]
    {
        let shell = if std::path::Path::new("/bin/bash").exists() {
            "/bin/bash"
        } else {
            "/bin/sh"
        };
        if let Ok(Ok(output)) = tokio::time::timeout(
            Duration::from_secs(3),
            {
                let mut c = AsyncCommand::new(shell);
                c.args(["-c", "node --version"]);
                hide_console(&mut c);
                c.output()
            },
        ).await {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
        }
    }

    Err("未检测到 Node.js".to_string())
}

pub async fn is_port_available(port: u16) -> bool {
    use std::net::TcpListener;
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// 检查本机端口是否已经有 TCP 服务在监听。
///
/// 这里不能使用 `TcpListener::bind` 反推服务状态：Windows 上的套接字复用、
/// 绑定地址差异等情况可能让 bind 成功，但浏览器已经可以连接该端口。
/// 直接连接端口与 PowerShell `Test-NetConnection` 的 TcpTestSucceeded 语义一致。
pub async fn is_tcp_port_open(port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_secs(1),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .map(|result| result.is_ok())
    .unwrap_or(false)
}

/// 端口探测结果
#[derive(Debug, PartialEq, Eq)]
pub enum PortProbe {
    /// 端口空闲，可以直接启动服务
    Free,
    /// 端口上已有 HTTP 服务在运行（如之前启动的 dsh），直接复用
    ServiceRunning,
    /// 端口被其他非服务程序占用，需要切换到下一个可用端口
    OccupiedByOther,
}

/// 探测指定端口上是否已有可用的 HTTP 服务。
/// 能建立 TCP 连接且收到 HTTP 响应头，即认为已有服务在运行；
/// 能连接但没有 HTTP 响应，判定为被其他程序占用；
/// 无法连接，判定为空闲。
pub async fn probe_port(port: u16) -> PortProbe {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("127.0.0.1:{}", port);
    let connect = tokio::time::timeout(
        Duration::from_secs(1),
        tokio::net::TcpStream::connect(&addr),
    )
    .await;
    let Ok(connect_result) = connect else {
        return PortProbe::Free;
    };
    let Ok(mut stream) = connect_result else {
        return PortProbe::Free;
    };

    let request = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(request.as_bytes()).await;

    let mut buf = [0u8; 64];
    let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
    match read {
        Ok(Ok(n)) if n > 0 => {
            let head = String::from_utf8_lossy(&buf[..n]).to_uppercase();
            if head.starts_with("HTTP/") {
                PortProbe::ServiceRunning
            } else {
                PortProbe::OccupiedByOther
            }
        }
        _ => PortProbe::OccupiedByOther,
    }
}

pub async fn find_available_port(start_port: u16) -> Result<u16, String> {
    let mut port = start_port;
    for _ in 0..10 {
        if is_port_available(port).await {
            return Ok(port);
        }
        port += 1;
    }
    Err(format!(
        "端口 {} 及后续 {} 个端口均被占用",
        start_port, 10
    ))
}

/// npx 启动方式：由于新版 Node.js 只提供 npx.cmd（无 npx.exe），且 npx.cmd 需要 cmd 解释器，
/// 直接 spawn .cmd 会报 os error 193（不是有效的 Win32 应用程序），还会弹 cmd 黑窗口。
/// 最稳的方式：找到 node.exe 绝对路径，用它直接运行 npm 自带的 npx-cli.js（无需 cmd，无窗口）。
pub enum NpxLauncher {
    /// node.exe + npx-cli.js（Windows 首选）
    NodeCli(PathBuf, PathBuf),
    /// 直接可执行文件（有 npx.exe 的老版本 Node，或非 Windows 平台的 npx）
    Direct(PathBuf),
}

pub fn find_npx_launcher() -> Option<NpxLauncher> {
    #[cfg(target_os = "windows")]
    {
        // 策略 1：where node → 拿到 node.exe → 推导同目录 node_modules/npm/bin/npx-cli.js
        if let Ok(output) = std::process::Command::new("where")
            .arg("node")
            .creation_flags(0x08000000)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(first) = text
                    .lines()
                    .map(|l| l.trim())
                    .find(|p| p.to_lowercase().ends_with(".exe"))
                {
                    let node = PathBuf::from(first);
                    let cli = node
                        .parent()?
                        .join("node_modules")
                        .join("npm")
                        .join("bin")
                        .join("npx-cli.js");
                    if cli.exists() {
                        return Some(NpxLauncher::NodeCli(node, cli));
                    }
                }
            }
        }

        // 策略 2：where npx → 如果有 npx.exe 则直接用
        if let Ok(output) = std::process::Command::new("where")
            .arg("npx")
            .creation_flags(0x08000000)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(exe) = text
                    .lines()
                    .map(|l| l.trim())
                    .find(|p| p.to_lowercase().ends_with(".exe"))
                {
                    return Some(NpxLauncher::Direct(PathBuf::from(exe)));
                }
            }
        }

        // 策略 3：常见 Node 安装目录硬编码（node.exe + npx-cli.js 或 npx.exe）
        for base in [
            r"D:\nodejs",
            r"C:\Program Files\nodejs",
            r"C:\Program Files (x86)\nodejs",
        ] {
            let node = PathBuf::from(base).join("node.exe");
            let cli = PathBuf::from(base)
                .join("node_modules")
                .join("npm")
                .join("bin")
                .join("npx-cli.js");
            if node.exists() && cli.exists() {
                return Some(NpxLauncher::NodeCli(node, cli));
            }
            let npx_exe = PathBuf::from(base).join("npx.exe");
            if npx_exe.exists() {
                return Some(NpxLauncher::Direct(npx_exe));
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // macOS/Linux：直接让 npx 走 PATH
        return Some(NpxLauncher::Direct(PathBuf::from("npx")));
    }
    None
}

#[derive(Debug, Clone)]
enum DshLauncher {
    #[cfg(target_os = "windows")]
    CmdShim(PathBuf),
    Direct(PathBuf),
}

#[cfg(target_os = "windows")]
fn dsh_launcher_from_path(path: PathBuf) -> Option<DshLauncher> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "cmd" | "bat" => Some(DshLauncher::CmdShim(path)),
        "exe" | "com" => Some(DshLauncher::Direct(path)),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
fn dsh_launcher_from_path(path: PathBuf) -> Option<DshLauncher> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = path.metadata().ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .then_some(DshLauncher::Direct(path))
}

fn npm_global_prefix() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/d", "/c", "npm", "prefix", "-g"]);
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        command
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut command = Command::new("npm");
        command.args(["prefix", "-g"]);
        command
    };

    let output = command.stdout(Stdio::piped()).stderr(Stdio::null()).output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .last()
        .map(PathBuf::from)
}

fn find_dsh_launcher() -> Option<DshLauncher> {
    #[cfg(target_os = "windows")]
    {
        let where_output = {
            let mut command = Command::new("where");
            command
                .arg("dsh")
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            command.output()
        };
        if let Ok(output) = where_output {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let path = PathBuf::from(line.trim());
                    if path.is_file() {
                        if let Some(launcher) = dsh_launcher_from_path(path) {
                            return Some(launcher);
                        }
                    }
                }
            }
        }

        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                for name in ["dsh.cmd", "dsh.exe", "dsh.com", "dsh.bat"] {
                    let candidate = dir.join(name);
                    if candidate.is_file() {
                        return dsh_launcher_from_path(candidate);
                    }
                }
            }
        }

        let mut bin_dirs = Vec::new();
        if let Some(app_data) = std::env::var_os("APPDATA") {
            bin_dirs.push(PathBuf::from(app_data).join("npm"));
        }
        if let Some(prefix) = npm_global_prefix() {
            bin_dirs.push(prefix);
        }
        for dir in bin_dirs {
            for name in ["dsh.cmd", "dsh.exe", "dsh.com", "dsh.bat"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return dsh_launcher_from_path(candidate);
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                let candidate = dir.join("dsh");
                if candidate.is_file() {
                    return dsh_launcher_from_path(candidate);
                }
            }
        }

        if let Some(prefix) = npm_global_prefix() {
            let candidate = prefix.join("bin").join("dsh");
            if candidate.is_file() {
                return dsh_launcher_from_path(candidate);
            }
        }
    }

    None
}

pub(crate) fn new_dsh_command() -> Option<Command> {
    let launcher = find_dsh_launcher()?;

    let mut command = match launcher {
        #[cfg(target_os = "windows")]
        DshLauncher::CmdShim(shim) => {
            let bin_dir = shim.parent()?.to_path_buf();
            let shim_name = shim.file_name()?.to_os_string();
            let mut command = Command::new("cmd");
            command.args(["/d", "/c"]).arg(shim_name);

            let mut paths = vec![bin_dir];
            if let Some(current_path) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&current_path));
            }
            if let Ok(path) = std::env::join_paths(paths) {
                command.env("PATH", path);
            }
            command
        }
        DshLauncher::Direct(path) => Command::new(path),
    };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    Some(command)
}

/// 给已构造好的 dsh `Command` 追加 `web --port <port> --no-open` 参数。
///
/// ⚠️ 端口必须用 `--port` 参数传递：dsh (`--profile web`) 只认这个 flag，
/// 不读 PORT 环境变量。旧版本用 `env("PORT", ...)` 会让 dsh 监听自己的默认端口，
/// 导致后端 `is_tcp_port_open(3080)` 永远连接失败 → 90 秒硬上限超时 →
/// 前端收不到 `service-ready` 事件 → 内嵌 webview 永远不显示（卡 loading）。
///
/// 验证方式：`dsh web --help` 可见 `--port <port>  listen port`；
/// npm readme 也只展示 `dsh --profile web --port 8080` 这种用法。
///
/// `--no-open` 顺手抑制 dsh 默认调起系统浏览器，避免和外层 iframe 体验冲突。
///
/// 拆成独立函数是为了方便单元测试（见 `tests::dsh_web_command_uses_port_flag`），
/// 直接验证参数契约，不依赖 dsh 是否真的安装。
fn configure_dsh_web_command(mut cmd: Command, port: u16) -> Command {
    cmd.arg("web");
    cmd.arg("--port").arg(port.to_string());
    cmd.arg("--no-open");
    cmd
}

fn spawn_dsh_process(install_dir: &PathBuf, port: u16) -> Result<Child, String> {
    let cmd = new_dsh_command().ok_or_else(|| "未检测到已安装的 dsh".to_string())?;
    let mut cmd = configure_dsh_web_command(cmd, port);
    cmd.current_dir(install_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    cmd.spawn().map_err(|e| format!("启动服务失败: {}", e))
}

/// Windows 下用 taskkill /T 杀掉整个进程树（cmd.exe → node.exe）
fn kill_process_tree(child: &mut Child) {
    #[cfg(target_os = "windows")]
    {
        let pid = child.id();
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .status();
        let _ = child.wait();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

async fn attach_service_process(
    process: &Arc<Mutex<Option<Child>>>,
    child: Child,
) {
    let mut guard = process.lock().await;
    if let Some(mut old) = guard.take() {
        kill_process_tree(&mut old);
    }
    *guard = Some(child);
}

async fn set_service_running(
    app_handle: &AppHandle,
    status: &Arc<Mutex<ServiceStatus>>,
    running: bool,
) {
    let mut st = status.lock().await;
    st.running = running;
    let _ = app_handle.emit_all("service-status", st.clone());
}

fn start_pipe_readers(
    app_handle: &AppHandle,
    install_dir: &PathBuf,
    port: u16,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
) {
    let app_handle = app_handle.clone();
    let install_dir = install_dir.clone();
    if let Some(stdout_pipe) = stdout {
        let handle = app_handle.clone();
        let d = install_dir.clone();
        tokio::task::spawn_blocking(move || {
            read_pipe(stdout_pipe, handle, port, &d, "stdout");
        });
    }
    if let Some(stderr_pipe) = stderr {
        let handle = app_handle.clone();
        let d = install_dir.clone();
        tokio::task::spawn_blocking(move || {
            read_pipe(stderr_pipe, handle, port, &d, "stderr");
        });
    }
}

/// 用 npm install -g（--verbose）全局安装依赖，让用户能看到下载全过程
async fn run_npm_install(
    app_handle: &AppHandle,
    install_dir: &PathBuf,
    port: u16,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut cmd = Command::new("cmd");
    #[cfg(target_os = "windows")]
    cmd.args([
        "/c", "npm", "install", "-g", "@deepseek-ai/dsh@latest",
        "--verbose", "--no-audit", "--no-fund",
    ]);

    #[cfg(not(target_os = "windows"))]
    let mut cmd = Command::new("npm");
    #[cfg(not(target_os = "windows"))]
    cmd.args(["install", "-g", "@deepseek-ai/dsh@latest", "--verbose", "--no-audit", "--no-fund"]);

    cmd.current_dir(install_dir)
        .env("npm_config_cache", get_npm_cache_dir(install_dir))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 npm install 失败: {}", e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    start_pipe_readers(app_handle, install_dir, port, stdout, stderr);

    let status = loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(st) = child
            .try_wait()
            .map_err(|e| format!("等待 npm install 退出失败: {}", e))?
        {
            break st;
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err(format!("npm install 失败（退出码: {}）", status))
    }
}

pub async fn start_service(
    app_handle: AppHandle,
    config: AppConfig,
) -> Result<(), String> {
    let install_dir = get_install_dir();
    crate::logger::log_to_file(
        &install_dir,
        "INFO",
        &format!("收到启动服务请求 (端口: {})", config.port),
    );

    // 端口上已有服务在运行时，直接接管显示，不重复启动新进程
    match probe_port(config.port).await {
        PortProbe::ServiceRunning => {
            crate::logger::log_to_file(
                &install_dir,
                "INFO",
                &format!(
                    "检测到端口 {} 已有服务在运行，直接使用现有服务",
                    config.port
                ),
            );
            let _ = app_handle.emit_all("service-log", format!(
                "[系统] 检测到端口 {} 已有服务在运行，直接使用现有服务，不再重复启动",
                config.port
            ));

            let status_manager = app_handle.state::<ServiceManager>();
            status_manager.reset_restart_count().await;
            status_manager.is_restarting.store(false, Ordering::SeqCst);
            {
                let mut status = status_manager.status.lock().await;
                status.running = true;
                status.port = config.port;
                status.url = format!("http://127.0.0.1:{}", config.port);
                let _ = app_handle.emit_all("service-status", status.clone());
            }
            // 关键修复：PortProbe::Running 路径下启动器没 spawn dsh 也没 read_pipe，
            // 无法拿到 token URL。如果 emit 裸 URL，前端会弹窗打开 dsh 但 dsh 会拒认证
            // （"authentication required; reopen the URL printed by dsh web"）。
            // 只更新状态不 emit service-ready，让用户去手动访问 dsh 自己打印的 URL。
            let _ = app_handle.emit_all(
                "service-log",
                format!(
                    "[系统] 检测到端口 {} 已有 dsh 在运行（不是启动器启动的）— 启动器无法获取其 token URL，\
                     请使用 dsh 自己打印的完整 URL（带 ?token=...）访问。已阻止前端弹出裸 URL 被 dsh 拒认证。",
                    config.port
                ),
            );
            crate::logger::log_to_file(
                &install_dir,
                "INFO",
                "PortProbe::Running: 跳过 service-ready emit（无 token URL）",
            );
            return Ok(());
        }
        PortProbe::OccupiedByOther => {
            crate::logger::log_to_file(
                &install_dir,
                "WARN",
                &format!(
                    "端口 {} 已被其他程序占用（无 HTTP 服务），尝试切换端口",
                    config.port
                ),
            );
        }
        PortProbe::Free => {}
    }

    let port = find_available_port(config.port).await?;
    crate::logger::log_to_file(&install_dir, "INFO", &format!("端口检测完成: {}", port));

    // 临时切换端口仅本次运行生效，不写回配置：
    // 若原先占用的端口之后被释放，下次启动仍会使用用户配置的端口
    if port != config.port {
        crate::logger::log_to_file(
            &install_dir,
            "WARN",
            &format!(
                "端口 {} 被其他程序占用，本次运行临时使用端口 {}（配置保持为 {}）",
                config.port, port, config.port
            ),
        );
        let _ = app_handle.emit_all("service-log", format!(
            "[系统] 端口 {} 被其他程序占用，本次运行临时使用端口 {}（配置仍保持为 {}，下次启动仍使用配置端口）",
            config.port, port, config.port
        ));
    }

    let status_manager = app_handle.state::<ServiceManager>();
    crate::logger::log_to_file(&install_dir, "INFO", "ServiceManager 状态获取完成");
    status_manager.reset_restart_count().await;
    crate::logger::log_to_file(&install_dir, "INFO", "重启计数已重置");
    status_manager.is_restarting.store(false, Ordering::SeqCst);

    {
        let mut status = status_manager.status.lock().await;
        status.port = port;
        status.url = format!("http://127.0.0.1:{}", port);
    }
    crate::logger::log_to_file(&install_dir, "INFO", "服务状态已更新");

    if find_dsh_launcher().is_none() {
        crate::logger::log_to_file(
            &install_dir,
            "INFO",
            "未检测到 dsh，开始执行 npm 全局安装",
        );
        let _ = app_handle.emit_all(
            "service-log",
            "[系统] 未检测到 dsh，正在执行: npm install -g @deepseek-ai/dsh@latest --verbose".to_string(),
        );

        if let Err(e) = run_npm_install(&app_handle, &install_dir, port).await {
            crate::logger::log_to_file(
                &install_dir,
                "ERROR",
                &format!("dsh 安装失败: {}", e),
            );
            let _ = app_handle.emit_all(
                "service-log",
                format!(
                    "[错误] dsh 安装失败: {}（若安装在系统目录，请尝试以管理员身份运行）",
                    e
                ),
            );
            return Err(e);
        }

        if find_dsh_launcher().is_none() {
            let error = "dsh 安装已完成，但仍无法定位 dsh 命令；请重启程序以刷新 PATH";
            crate::logger::log_to_file(&install_dir, "ERROR", error);
            let _ = app_handle.emit_all("service-log", format!("[错误] {}", error));
            return Err(error.to_string());
        }

        crate::logger::log_to_file(&install_dir, "INFO", "dsh 安装完成");
        let _ = app_handle.emit_all(
            "service-log",
            "[系统] dsh 安装完成，正在执行: dsh web".to_string(),
        );
    } else {
        crate::logger::log_to_file(&install_dir, "INFO", "已检测到 dsh，直接启动服务");
        let _ = app_handle.emit_all(
            "service-log",
            "[系统] 已检测到 dsh，正在执行: dsh web".to_string(),
        );
    }

    let mut child = spawn_dsh_process(&install_dir, port).map_err(|e| {
        crate::logger::log_to_file(&install_dir, "ERROR", &e);
        e
    })?;
    crate::logger::log_to_file(&install_dir, "INFO", "dsh 子进程已生成");
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child_pid = child.id();

    attach_service_process(&status_manager.process, child).await;
    set_service_running(&app_handle, &status_manager.status, true).await;

    let _ = app_handle.emit_all("service-log", format!(
        "[系统] 服务已启动 (PID: {}, 端口: {})",
        child_pid, port
    ));
    crate::logger::log_to_file(
        &install_dir,
        "INFO",
        &format!("服务进程已启动 (PID: {}, 端口: {})", child_pid, port),
    );

    start_pipe_readers(&app_handle, &install_dir, port, stdout, stderr);

    // 等待服务就绪（最长 90 秒）
    let handle_ready = app_handle.clone();
    tokio::spawn(async move {
        let mut ready = false;
        const MAX_WAIT: u64 = 90;
        for i in 0..MAX_WAIT {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if is_tcp_port_open(port).await {
                ready = true;
                break;
            }
            if (i + 1) % 5 == 0 {
                let _ = handle_ready.emit_all("service-log", format!(
                    "[系统] 等待服务就绪... ({}/{})",
                    i + 1,
                    MAX_WAIT
                ));
            }
        }
        let install_dir = get_install_dir();
        if ready {
            // 优先等 read_pipe 线程捕获到带 token 的认证 URL（最多 15 秒），
            // 避免发出裸 URL 导致 dsh 返回 "authentication required"。
            // 15 秒而非 8 秒：Node 子进程在 stdout 被 pipe（非 TTY）时会缓冲输出，
            // token URL 行可能迟到，给足时间避免被漏掉。
            let status_manager = handle_ready.state::<ServiceManager>();
            for _ in 0..15 {
                if status_manager.ready_emitted.load(Ordering::SeqCst) {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            // 防重复：若 read_pipe 已发送则跳过
            if status_manager.ready_emitted.swap(true, Ordering::SeqCst) {
                return;
            }
            let url = status_manager
                .auth_url
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| format!("http://127.0.0.1:{}", port));
            let _ = handle_ready.emit_all("service-ready", url.clone());
            auto_hide_main_window(&handle_ready);
            let _ = handle_ready.emit_all("service-log", "[系统] 服务已就绪".to_string());
            crate::logger::log_to_file(&install_dir, "INFO", "服务已就绪");
        } else {
            let _ = handle_ready.emit_all("service-log", format!(
                "[错误] 服务启动超时（{} 秒），端口未能就绪",
                MAX_WAIT
            ));
            crate::logger::log_to_file(&install_dir, "ERROR", "服务启动超时，端口未能就绪");
        }
    });

    // Start monitor if not already running
    if !status_manager.monitor_running.load(Ordering::SeqCst) {
        status_manager.monitor_running.store(true, Ordering::SeqCst);
        let process = status_manager.process.clone();
        let status = status_manager.status.clone();
        let restart_count = status_manager.restart_count.clone();
        let is_restarting = status_manager.is_restarting.clone();
        let install_dir_clone = status_manager.install_dir.clone();
        spawn_monitor(
            app_handle,
            process,
            status,
            restart_count,
            is_restarting,
            install_dir_clone,
        );
    }

    Ok(())
}

fn spawn_monitor(
    app_handle: AppHandle,
    process: Arc<Mutex<Option<Child>>>,
    status: Arc<Mutex<ServiceStatus>>,
    restart_count: Arc<Mutex<u32>>,
    is_restarting: Arc<AtomicBool>,
    install_dir: PathBuf,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;

            let mut proc = process.lock().await;

            if let Some(child) = proc.as_mut() {
                match child.try_wait() {
                    Ok(Some(exit_status)) => {
                        crate::logger::log_to_file(
                            &install_dir,
                            "WARN",
                            &format!("服务进程已退出 (状态: {})", exit_status),
                        );
                        let _ = app_handle.emit_all(
                            "service-log",
                            format!(
                                "[系统] 服务进程已退出 (状态: {})",
                                exit_status
                            ),
                        );
                        drop(proc);

                        if is_restarting.load(Ordering::SeqCst) {
                            continue;
                        }

                        let mut count = restart_count.lock().await;
                        if *count < 3 {
                            *count += 1;
                            is_restarting.store(true, Ordering::SeqCst);

                            let _ = app_handle.emit_all(
                                "service-log",
                                format!(
                                    "[系统] 5秒后自动重启服务 (第 {} 次)...",
                                    *count
                                ),
                            );

                            let mut st = status.lock().await;
                            st.running = false;
                            let _ = app_handle.emit_all("service-status", st.clone());
                            drop(st);
                            drop(count);

                            tokio::time::sleep(Duration::from_secs(5)).await;

                            let cfg = AppConfig::load(&install_dir).unwrap_or_default();
                            let result = start_service_internal(
                                app_handle.clone(),
                                cfg,
                                process.clone(),
                                status.clone(),
                                install_dir.clone(),
                            ).await;

                            is_restarting.store(result.is_err(), Ordering::SeqCst);

                            if let Err(e) = result {
                                crate::logger::log_to_file(
                                    &install_dir,
                                    "ERROR",
                                    &format!("重启服务失败: {}", e),
                                );
                                let _ = app_handle.emit_all(
                                    "service-log",
                                    format!("[错误] 重启服务失败: {}", e),
                                );
                            }
                        } else {
                            let mut st = status.lock().await;
                            st.running = false;
                            let _ = app_handle.emit_all("service-status", st.clone());
                            drop(st);
                            crate::logger::log_to_file(
                                &install_dir,
                                "ERROR",
                                "自动重启已达最大次数，服务已停止（监控循环退出）",
                            );
                            let _ = app_handle.emit_all(
                                "service-log",
                                "[系统] 自动重启已达最大次数，服务已停止（监控循环退出）".to_string(),
                            );
                            // 关键修复：break 跳出 monitor loop，整个 async task 死亡，proc MutexGuard 自动 drop，
                            // process 锁自然释放，无需手动清空（也清不了——match arm 内 `proc.as_mut()` 借用了
                            // proc 到 arm 结束，borrow check 阻止 mutate）。
                            // 之前每 1 秒重复打一次 error 日志（"日志爆炸"）是因为没 break，下个循环又 try_wait
                            // 同一个已退出 child 又触发同一个 else 分支。
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
        }
    });
}

async fn start_service_internal(
    app_handle: AppHandle,
    config: AppConfig,
    process: Arc<Mutex<Option<Child>>>,
    status: Arc<Mutex<ServiceStatus>>,
    install_dir: PathBuf,
) -> Result<(), String> {
    // 关键修复：服务重启（手动 restart / monitor 自动重启）时清空上次的 auth_url + 重置 ready_emitted CAS，
    // 否则 read_pipe 抓到新 token URL 会被旧 CAS 跳过 emit → 前端永远不重弹 webview → 拒绝访问（用了旧 URL）。
    let status_manager = app_handle.state::<ServiceManager>();
    status_manager.reset_ready_state().await;

    let port = find_available_port(config.port).await?;

    if port != config.port {
        crate::logger::log_to_file(
            &install_dir,
            "WARN",
            &format!(
                "端口 {} 被其他程序占用，本次运行临时使用端口 {}（配置保持为 {}）",
                config.port, port, config.port
            ),
        );
        let _ = app_handle.emit_all("service-log", format!(
            "[系统] 端口 {} 被其他程序占用，本次运行临时使用端口 {}（配置仍保持为 {}）",
            config.port, port, config.port
        ));
    }

    {
        let mut st = status.lock().await;
        st.port = port;
        st.url = format!("http://127.0.0.1:{}", port);
    }

    let mut child = spawn_dsh_process(&install_dir, port).map_err(|e| {
        crate::logger::log_to_file(&install_dir, "ERROR", &e);
        e
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child_pid = child.id();

    attach_service_process(&process, child).await;
    set_service_running(&app_handle, &status, true).await;

    let _ = app_handle.emit_all("service-log", format!(
        "[系统] 服务已启动 (PID: {}, 端口: {})",
        child_pid, port
    ));
    crate::logger::log_to_file(
        &install_dir,
        "INFO",
        &format!("服务进程已启动 (PID: {}, 端口: {})", child_pid, port),
    );

    start_pipe_readers(&app_handle, &install_dir, port, stdout, stderr);

    let handle_ready = app_handle.clone();
    tokio::spawn(async move {
        let mut ready = false;
        const MAX_WAIT: u64 = 90;
        for i in 0..MAX_WAIT {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if is_tcp_port_open(port).await {
                ready = true;
                break;
            }
            if (i + 1) % 5 == 0 {
                let _ = handle_ready.emit_all("service-log", format!(
                    "[系统] 等待服务就绪... ({}/{})",
                    i + 1,
                    MAX_WAIT
                ));
            }
        }
        let install_dir = get_install_dir();
        if ready {
            // 优先等 read_pipe 线程捕获到带 token 的认证 URL（最多 15 秒），
            // 避免发出裸 URL 导致 dsh 返回 "authentication required"。
            // 15 秒而非 8 秒：Node 子进程在 stdout 被 pipe（非 TTY）时会缓冲输出，
            // token URL 行可能迟到，给足时间避免被漏掉。
            let status_manager = handle_ready.state::<ServiceManager>();
            for _ in 0..15 {
                if status_manager.ready_emitted.load(Ordering::SeqCst) {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            // 防重复：若 read_pipe 已发送则跳过
            if status_manager.ready_emitted.swap(true, Ordering::SeqCst) {
                return;
            }
            let url = status_manager
                .auth_url
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| format!("http://127.0.0.1:{}", port));
            let _ = handle_ready.emit_all("service-ready", url.clone());
            auto_hide_main_window(&handle_ready);
            let _ = handle_ready.emit_all("service-log", "[系统] 服务已就绪".to_string());
            crate::logger::log_to_file(&install_dir, "INFO", "服务已就绪");
        } else {
            let _ = handle_ready.emit_all("service-log", format!(
                "[错误] 服务启动超时（{} 秒），端口未能就绪",
                MAX_WAIT
            ));
            crate::logger::log_to_file(&install_dir, "ERROR", "服务启动超时，端口未能就绪");
        }
    });

    Ok(())
}

pub async fn stop_service(app_handle: AppHandle) -> Result<(), String> {
    let status_manager = app_handle.state::<ServiceManager>();

    status_manager.is_restarting.store(true, Ordering::SeqCst);

    let mut process = status_manager.process.lock().await;
    let has_owned_process = process.is_some();
    if let Some(mut child) = process.take() {
        let pid = child.id();
        // Windows 下用 taskkill /T 杀掉整个进程树
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .status();
            let _ = child.wait();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    drop(process);

    {
        let mut status = status_manager.status.lock().await;
        status.running = false;
        let _ = app_handle.emit_all("service-status", status.clone());
    }

    let message = if has_owned_process {
        "[系统] 服务已停止".to_string()
    } else {
        "[系统] 未检测到本程序启动的服务（端口上的外部服务不受本程序管理）".to_string()
    };
    let _ = app_handle.emit_all("service-log", message.clone());
    crate::logger::log_to_file(
        &get_install_dir(),
        if has_owned_process { "INFO" } else { "WARN" },
        if has_owned_process {
            "服务已停止"
        } else {
            "停止请求忽略：未检测到本程序启动的服务进程"
        },
    );
    Ok(())
}

/// 从 dsh web 的 stdout 行中提取访问 URL。
///
/// 新版 dsh 默认启用 URL token 认证：启动后打印形如
/// `dsh web: http://127.0.0.1:3080/?token=abc123`，必须把整串（含 `?token=` 查询参数）
/// 原样交给前端 iframe，否则 dsh 会返回 "authentication required; reopen the URL printed by dsh web"。
///
/// 旧版 dsh 打印不带 token 的 `http://127.0.0.1:3080`，这里同样能提取（兼容）。
/// 同时兼容 `localhost` 写法与多端口（不硬编码 3080）。
///
/// 注意：只匹配 `127.0.0.1` / `localhost` 的 http URL，避免误抓日志里其它 http 链接。
fn extract_dsh_url(line: &str) -> Option<String> {
    for prefix in ["http://127.0.0.1:", "http://localhost:"] {
        if let Some(idx) = line.find(prefix) {
            let rest = &line[idx..];
            // 截到空白或常见引号/尖括号为止，避免把行尾无关字符带进 URL
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>')
                .unwrap_or(rest.len());
            let candidate = &rest[..end];
            if candidate.starts_with("http://") && candidate.contains(':') {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

/// 剥掉 ANSI 转义序列（如 `\x1b[36m...\x1b[0m` 颜色码）。
/// dsh 可能给 `dsh web:` 等输出上色，转义码插在文本里会让 `extract_dsh_url`
/// 的子串查找误判，先剥干净再提取更稳。
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // 跳过 ESC[ ... 直到一个字母（CSI 序列结束符）
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn read_pipe<R: Read>(
    pipe: R,
    app_handle: AppHandle,
    port: u16,
    install_dir: &PathBuf,
    stream_type: &str,
) {
    let reader = BufReader::new(pipe);
    let log_dir = install_dir.join("config").join("npmlog");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file_path =
        log_dir.join(format!("npm-{}.log", chrono::Local::now().format("%Y-%m-%d")));

    for line in reader.lines() {
        match line {
            Ok(line) => {
                let timestamp = chrono::Local::now()
                    .format("%H:%M:%S%.3f")
                    .to_string();
                let log_line = format!(
                    "[{}] [{}] [port:{}] {}",
                    timestamp, stream_type, port, line
                );

                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_file_path)
                {
                    let _ = writeln!(file, "{}", log_line);
                }

                let _ = app_handle.emit_all("service-log", log_line);

                // 捕获 dsh web 打印的带 token 认证 URL，交给前端 iframe 使用。
                // 详见 `extract_dsh_url` 注释：新版 dsh 必须带 ?token= 才能通过认证。
                // 先剥掉 ANSI 转义码（dsh 可能给输出上色），避免干扰 URL 提取。
                let clean_line = strip_ansi(&line);
                if let Some(auth_url) = extract_dsh_url(&clean_line) {
                    // ★ 明确把抓取到的完整 URL 写进日志文件，便于在 npmlog 中验证抓取是否生效。
                    // dsh 原样输出行已经在上方写过了，这里再补一行「捕获确认」，两行对照一目了然。
                    if let Ok(mut cap_file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_file_path)
                    {
                        let _ = writeln!(
                            cap_file,
                            "[{}] [捕获] 已提取 dsh 认证 URL（将发送给前端 iframe）: {}",
                            timestamp, auth_url
                        );
                    }
                    // 同时写一份到 applog，双保险
                    crate::logger::log_to_file(
                        &install_dir,
                        "INFO",
                        &format!("已提取 dsh 认证 URL（将发送给前端 iframe）: {}", auth_url),
                    );
                    let sm = app_handle.state::<ServiceManager>();
                    if let Ok(mut g) = sm.auth_url.lock() {
                        *g = Some(auth_url.clone());
                    }
                    // 首次捕获即视为服务就绪，立即通知前端（带 token 的 URL）
                    if !sm.ready_emitted.swap(true, Ordering::SeqCst) {
                        let _ = app_handle.emit_all("service-ready", auth_url.clone());
                        let _ = app_handle.emit_all(
                            "service-log",
                            "[系统] 已捕获 dsh 认证 URL，服务已就绪".to_string(),
                        );
                        auto_hide_main_window(&app_handle);
                    }
                } else if clean_line.contains("token=") {
                    // dsh 输出了 token 但没能提取成 URL（格式变化等），报警便于排查
                    let _ = app_handle.emit_all(
                        "service-log",
                        format!("[调试] dsh 输出含 token= 但未能提取 URL，原始行: {}", line),
                    );
                    // 同样写进日志文件，方便用户直接看到这种边界情况
                    let _ = crate::logger::log_to_file(
                        &install_dir,
                        "DEBUG",
                        &format!("dsh 输出含 token= 但未能提取 URL，原始行: {}", line),
                    );
                }
            }
            Err(_) => break,
        }
    }
}

#[tauri::command]
pub async fn start_harness_service(
    app_handle: AppHandle,
    config: AppConfig,
) -> Result<(), String> {
    start_service(app_handle, config).await
}

#[tauri::command]
pub async fn stop_harness_service(app_handle: AppHandle) -> Result<(), String> {
    stop_service(app_handle).await
}

#[tauri::command]
pub async fn get_service_status(
    app_handle: AppHandle,
) -> Result<ServiceStatus, String> {
    let state = app_handle.state::<ServiceManager>();
    let status = state.status.lock().await.clone();
    Ok(status)
}

#[tauri::command]
pub async fn check_nodejs() -> Result<String, String> {
    let result = check_node().await;
    let install_dir = get_install_dir();
    match &result {
        Ok(version) => {
            crate::logger::log_to_file(&install_dir, "INFO", &format!("Node.js 检测成功: {}", version));
        }
        Err(e) => {
            crate::logger::log_to_file(&install_dir, "ERROR", &format!("Node.js 检测失败: {}", e));
        }
    }
    result
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{dsh_launcher_from_path, DshLauncher};
    use std::path::PathBuf;

    #[test]
    fn recognizes_windows_dsh_launchers() {
        assert!(matches!(
            dsh_launcher_from_path(PathBuf::from(r"C:\npm\dsh.cmd")),
            Some(DshLauncher::CmdShim(_))
        ));
        assert!(matches!(
            dsh_launcher_from_path(PathBuf::from(r"C:\npm\dsh.exe")),
            Some(DshLauncher::Direct(_))
        ));
    }

    #[test]
    fn ignores_non_windows_shims() {
        assert!(dsh_launcher_from_path(PathBuf::from(r"C:\npm\dsh")).is_none());
        assert!(dsh_launcher_from_path(PathBuf::from(r"C:\npm\dsh.ps1")).is_none());
    }
}

/// 参数契约回归测试：覆盖「构造 dsh web 命令」的 bug。
///
/// 历史教训：旧版 `spawn_dsh_process` 用 `cmd.env("PORT", port)` 传端口，
/// 但 dsh (`--profile web`) **不读 PORT 环境变量**，只认 `--port` 参数，
/// 导致 dsh 监听默认端口 → 启动器 90 秒内 TCP 探测 3080 永远失败 → 前端卡 loading。
///
/// 这个测试用 `Command::new("dsh")` 绕过 `new_dsh_command` 的查找逻辑，
/// 只验证参数拼接契约，跨平台都能跑、不依赖 dsh 是否真安装。
#[cfg(test)]
mod dsh_command_args_tests {
    use super::{configure_dsh_web_command, extract_dsh_url};
    use std::process::Command;

    fn args_of(cmd: &Command) -> Vec<&str> {
        cmd.get_args().filter_map(|a| a.to_str()).collect()
    }

    #[test]
    fn dsh_web_command_uses_port_flag_not_env() {
        let cmd = configure_dsh_web_command(Command::new("dsh"), 3080);
        let args = args_of(&cmd);

        // 1) 必须包含 --port 和端口号（核心契约：这是修复的核心）
        assert!(
            args.contains(&"--port"),
            "dsh 必须接收 --port 参数（不读 PORT 环境变量），实际 args: {:?}",
            args
        );
        assert!(
            args.contains(&"3080"),
            "端口号必须作为独立 token 出现，实际 args: {:?}",
            args
        );

        // 2) 必须包含 web 子命令
        assert!(args.contains(&"web"), "必须传 web 子命令，实际 args: {:?}", args);

        // 3) 必须包含 --no-open 抑制默认浏览器弹窗
        assert!(
            args.contains(&"--no-open"),
            "必须传 --no-open 抑制 dsh 默认开浏览器，实际 args: {:?}",
            args
        );

        // 4) 严格顺序：web --port <port> --no-open（避免被人插 flag 打断）
        let web_idx = args.iter().position(|&a| a == "web").expect("web 子命令缺失");
        assert_eq!(
            args.get(web_idx + 1).copied(),
            Some("--port"),
            "--port 必须紧跟 web 之后，实际 args: {:?}",
            args
        );
        assert_eq!(
            args.get(web_idx + 2).copied(),
            Some("3080"),
            "端口号必须紧跟 --port 之后，实际 args: {:?}",
            args
        );
        assert_eq!(
            args.get(web_idx + 3).copied(),
            Some("--no-open"),
            "--no-open 必须紧跟端口号之后，实际 args: {:?}",
            args
        );
    }

    #[test]
    fn dsh_web_command_honors_custom_port() {
        // 防止「写死 3080」之类的退化：用户改 config.toml 端口后必须能传出去
        let cmd = configure_dsh_web_command(Command::new("dsh"), 1420);
        let args = args_of(&cmd);
        assert!(args.contains(&"1420"), "自定义端口必须被透传，实际 args: {:?}", args);
        assert!(
            !args.contains(&"3080"),
            "传非 3080 端口时不应出现 3080 字面量，实际 args: {:?}",
            args
        );
    }

    #[test]
    fn dsh_url_extract_captures_token_url() {
        // 新版 dsh：必须整串（含 ?token=）原样提取，否则前端认证失败
        assert_eq!(
            extract_dsh_url("dsh web: http://127.0.0.1:3080/?token=abc123XYZ"),
            Some("http://127.0.0.1:3080/?token=abc123XYZ".to_string())
        );
    }

    #[test]
    fn dsh_url_extract_captures_plain_url_without_token() {
        // 旧版 dsh：无 token 也能提取（兼容）
        assert_eq!(
            extract_dsh_url("dsh web: http://127.0.0.1:3080"),
            Some("http://127.0.0.1:3080".to_string())
        );
    }

    #[test]
    fn dsh_url_extract_keeps_underscore_in_token() {
        // 真实 dsh 打印的 token 含下划线（如 l13aTokKJe0os8tAIWGJ19V7eX_v6DyktXD4ydUT_2k），
        // 截断逻辑不能把 `_` 当空白切掉，否则前端 iframe 用残缺 token 会认证失败。
        assert_eq!(
            extract_dsh_url(
                "dsh web: http://127.0.0.1:3080/?token=l13aTokKJe0os8tAIWGJ19V7eX_v6DyktXD4ydUT_2k"
            ),
            Some(
                "http://127.0.0.1:3080/?token=l13aTokKJe0os8tAIWGJ19V7eX_v6DyktXD4ydUT_2k"
                    .to_string()
            )
        );
    }

    #[test]
    fn dsh_url_extract_handles_localhost_and_stops_at_whitespace() {
        assert_eq!(
            extract_dsh_url("dsh web: http://localhost:3080/?token=xyz and text"),
            Some("http://localhost:3080/?token=xyz".to_string())
        );
    }

    #[test]
    fn dsh_url_extract_ignores_unrelated_lines() {
        assert_eq!(
            extract_dsh_url("npm warn deprecated node-domexception@1.0.0"),
            None
        );
        // 非 127.0.0.1/localhost:port 的 http 链接不应被误抓
        assert_eq!(
            extract_dsh_url("see http://example.com/index for docs"),
            None
        );
    }
}
