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
    pub fn kill_service_blocking(&self) {
        self.is_restarting.store(true, Ordering::SeqCst);
        let mut process = self.process.blocking_lock();
        if let Some(mut child) = process.take() {
            let pid = child.id();
            // Windows 下用 taskkill /T 杀掉整个进程树（cmd.exe → node.exe）
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

fn spawn_dsh_process(install_dir: &PathBuf, port: u16) -> Result<Child, String> {
    // Windows 下通过 cmd /c 启动，确保能找到 npx（GUI 应用 PATH 可能不完整）
    #[cfg(target_os = "windows")]
    let mut cmd = Command::new("cmd");
    #[cfg(target_os = "windows")]
    cmd.args(["/c", "npx", "--yes", "@deepseek-ai/dsh", "web"]);

    #[cfg(not(target_os = "windows"))]
    let mut cmd = Command::new("npx");
    #[cfg(not(target_os = "windows"))]
    cmd.arg("@deepseek-ai/dsh").arg("web");

    cmd.env("PORT", port.to_string())
        .current_dir(install_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
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
        "/c", "npm", "install", "-g", "@deepseek-ai/dsh",
        "--verbose", "--no-audit", "--no-fund",
    ]);

    #[cfg(not(target_os = "windows"))]
    let mut cmd = Command::new("npm");
    #[cfg(not(target_os = "windows"))]
    cmd.args(["install", "-g", "@deepseek-ai/dsh", "--verbose", "--no-audit", "--no-fund"]);

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
    let port = find_available_port(config.port).await?;
    crate::logger::log_to_file(&install_dir, "INFO", &format!("端口检测完成: {}", port));

    if port != config.port {
        let mut new_config = config.clone();
        new_config.port = port;
        new_config.save(&install_dir)?;
        crate::logger::log_to_file(
            &install_dir,
            "INFO",
            &format!("端口 {} 被占用，已切换到端口 {}", config.port, port),
        );
        let _ = app_handle.emit_all("service-log", format!(
            "[系统] 端口 {} 被占用，已自动切换到端口 {} 并更新配置",
            config.port, port
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
        let mut new_config = config.clone();
        new_config.port = port;
        new_config.save(&install_dir)?;
        crate::logger::log_to_file(
            &install_dir,
            "INFO",
            &format!("端口 {} 被占用，已切换到端口 {}", config.port, port),
        );
        let _ = app_handle.emit_all("service-log", format!(
            "[系统] 端口 {} 被占用，已自动切换到端口 {} 并更新配置",
            config.port, port
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

    let _ = app_handle.emit_all("service-log", "[系统] 服务已停止".to_string());
    crate::logger::log_to_file(&get_install_dir(), "INFO", "服务已停止");
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
