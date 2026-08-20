use crate::commands::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::process::Command as AsyncCommand;
use tokio::sync::Mutex;

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
        }
    }

    pub async fn reset_restart_count(&self) {
        let mut count = self.restart_count.lock().await;
        *count = 0;
    }

    /// 同步杀掉服务进程（退出时调用）
    /// 关键优化：taskkill 改用 spawn 不再等 status，避免在 dsh 复杂进程树（cmd → npx → node → ...）下长时间阻塞
    /// 整体加 2 秒硬上限，超时就不等了——进程会被系统或 taskkill /F 异步清理
    pub fn kill_service_blocking(&self) {
        self.is_restarting.store(true, Ordering::SeqCst);
        let mut process = self.process.blocking_lock();
        if let Some(mut child) = process.take() {
            let pid = child.id();
            #[cfg(target_os = "windows")]
            {
                // 用 spawn 异步发起 taskkill，不 wait status（taskkill /F 在大型进程树上偶尔会卡几秒）
                let _ = std::process::Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                // 给子进程最多 2 秒时间退出，超时不再等（避免退出很慢）
                let start = std::time::Instant::now();
                while start.elapsed() < std::time::Duration::from_secs(2) {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        _ => std::thread::sleep(std::time::Duration::from_millis(50)),
                    }
                }
                // 即使超时也不 wait()——避免 hang 在 av/ntdll 锁
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

pub fn get_install_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub async fn check_node() -> Result<String, String> {
    // 策略1：直接调用 node（PATH 中有 node 时生效）
    if let Ok(Ok(output)) = tokio::time::timeout(
        Duration::from_secs(3),
        AsyncCommand::new("node").arg("--version").output(),
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
            AsyncCommand::new("cmd").args(["/c", "node", "--version"]).output(),
        ).await {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
        }

        // 策略3：用 where 命令查找 node 路径
        if let Ok(Ok(where_output)) = tokio::time::timeout(
            Duration::from_secs(3),
            AsyncCommand::new("where").arg("node").output(),
        ).await {
            if where_output.status.success() {
                let paths = String::from_utf8_lossy(&where_output.stdout);
                if let Some(first_line) = paths.lines().next() {
                    let node_path = first_line.trim();
                    if let Ok(Ok(output)) = tokio::time::timeout(
                        Duration::from_secs(3),
                        AsyncCommand::new(node_path).arg("--version").output(),
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
                AsyncCommand::new(path).arg("--version").output(),
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
            AsyncCommand::new(shell).args(["-c", "node --version"]).output(),
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

fn spawn_dsh_process(install_dir: &PathBuf, port: u16) -> Result<Child, String> {
    let launcher = find_npx_launcher().ok_or_else(|| "未找到 npx，请先安装 Node.js".to_string())?;

    // 用 node.exe 直跑 npx-cli.js（无需 cmd /c，彻底消除 cmd 黑窗口闪现，也避免 193 错误）
    let mut cmd = match launcher {
        NpxLauncher::NodeCli(node, cli) => {
            let mut c = Command::new(node);
            c.arg(cli);
            c
        }
        NpxLauncher::Direct(npx) => Command::new(npx),
    };
    cmd.args(["--yes", "@deepseek-ai/dsh@latest", "web"]);

    cmd.env("PORT", port.to_string())
        .current_dir(install_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW(0x08000000): 确保不会创建控制台窗口
        cmd.creation_flags(0x08000000);
    }

    cmd.spawn().map_err(|e| format!("启动服务失败: {}", e))
}

/// 轮询端口，端口被占用即视为服务就绪
async fn wait_port_ready(port: u16, timeout_secs: u64) -> bool {
    for _ in 0..timeout_secs {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if !is_port_available(port).await {
            return true;
        }
    }
    false
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
            let url = format!("http://127.0.0.1:{}", config.port);
            let _ = app_handle.emit_all("service-ready", url);
            let _ = app_handle.emit_all(
                "service-log",
                "[系统] 服务已就绪（复用现有服务）".to_string(),
            );
            crate::logger::log_to_file(&install_dir, "INFO", "服务已就绪（复用现有服务）");
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

    // 第一次尝试：直接 npx 启动（包已安装/已缓存时会很快）
    let _ = app_handle.emit_all(
        "service-log",
        "[系统] 尝试直接使用 npx 启动服务（若 10 秒内未就绪将自动切换为 npm 安装）".to_string(),
    );
    let mut child = spawn_dsh_process(&install_dir, port).map_err(|e| {
        crate::logger::log_to_file(&install_dir, "ERROR", &e);
        e
    })?;
    crate::logger::log_to_file(&install_dir, "INFO", "npx 子进程已生成");
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

    // 若 10 秒内服务未就绪，说明 npx 正在下载依赖
    // （npx 自身不支持 verbose，用户看不到进度，改用 npm install 显示下载全过程）
    if !wait_port_ready(port, 10).await {
        crate::logger::log_to_file(
            &install_dir,
            "WARN",
            "npx 运行超过 10 秒未就绪，疑似正在下载依赖，改用 npm install 安装",
        );
        let _ = app_handle.emit_all(
            "service-log",
            "[系统] npx 运行超过 10 秒未就绪，判定需要下载依赖，正在切换到 npm 全局安装...".to_string(),
        );

        // 防止监控线程把被杀掉的进程当成崩溃而自动重启
        status_manager.is_restarting.store(true, Ordering::SeqCst);

        {
            let mut guard = status_manager.process.lock().await;
            if let Some(mut old) = guard.take() {
                kill_process_tree(&mut old);
            }
        }
        set_service_running(&app_handle, &status_manager.status, false).await;

        let _ = app_handle.emit_all(
            "service-log",
            "[系统] 正在执行: npm install -g @deepseek-ai/dsh --verbose（安装日志将实时显示如下）请耐心等待，不要关闭程序...".to_string(),
        );

        match run_npm_install(&app_handle, &install_dir, port).await {
            Ok(()) => {
                crate::logger::log_to_file(&install_dir, "INFO", "依赖安装完成");
                let _ = app_handle.emit_all(
                    "service-log",
                    "[系统] 依赖安装完成，正在启动服务...".to_string(),
                );

                status_manager.is_restarting.store(false, Ordering::SeqCst);

                let mut child = spawn_dsh_process(&install_dir, port).map_err(|e| {
                    crate::logger::log_to_file(&install_dir, "ERROR", &e);
                    e
                })?;
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
            }
            Err(e) => {
                crate::logger::log_to_file(&install_dir, "ERROR", &format!("依赖安装失败: {}", e));
                let _ = app_handle.emit_all("service-log", format!(
                    "[错误] 依赖安装失败: {}（若安装在系统目录，请尝试以管理员身份运行）",
                    e
                ));
                return Err(e);
            }
        }
    }

    // 等待服务就绪（最长 90 秒，首次安装完成后 npx 会较快启动）
    let handle_ready = app_handle.clone();
    let url = format!("http://127.0.0.1:{}", port);
    tokio::spawn(async move {
        let mut ready = false;
        const MAX_WAIT: u64 = 90;
        for i in 0..MAX_WAIT {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !is_port_available(port).await {
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
            let _ = handle_ready.emit_all("service-ready", url.clone());
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
                            crate::logger::log_to_file(
                                &install_dir,
                                "ERROR",
                                "自动重启已达最大次数，服务已停止",
                            );
                            let _ = app_handle.emit_all(
                                "service-log",
                                "[系统] 自动重启已达最大次数，服务已停止".to_string(),
                            );
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
    let url = format!("http://127.0.0.1:{}", port);
    tokio::spawn(async move {
        let mut ready = false;
        const MAX_WAIT: u64 = 90;
        for i in 0..MAX_WAIT {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !is_port_available(port).await {
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
            let _ = handle_ready.emit_all("service-ready", url.clone());
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
