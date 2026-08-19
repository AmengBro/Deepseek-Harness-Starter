mod commands;
mod logger;

use commands::service::{get_install_dir, ServiceManager};
use commands::config::AppConfig;
use tauri::{
    api::path::home_dir, CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu,
    SystemTrayMenuItem, SystemTraySubmenu,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if !ensure_single_instance() {
        std::process::exit(0);
    }

    tauri::Builder::default()
        .setup(|app| {
            let install_dir = get_install_dir();

            let config_dir = install_dir.join("config");
            std::fs::create_dir_all(&config_dir).ok();
            std::fs::create_dir_all(&config_dir.join("applog")).ok();
            std::fs::create_dir_all(&config_dir.join("npmlog")).ok();

            logger::init(&install_dir)?;
            logger::set_app_handle(app.handle().clone());
            logger::cleanup_old_logs(&install_dir, 30);
            logger::log_to_file(&install_dir, "INFO", "应用启动");

            let _config = AppConfig::load(&install_dir).unwrap_or_default();

            let service_manager = ServiceManager::new(install_dir.clone());
            app.handle().manage(service_manager);

            Ok(())
        })
        .system_tray(
            SystemTray::new()
                .with_icon(tauri::Icon::Raw(include_bytes!("../icons/32x32.png").to_vec()))
                .with_menu(
                    SystemTrayMenu::new()
                        .add_item(CustomMenuItem::new("open".to_string(), "打开主界面"))
                        .add_submenu(SystemTraySubmenu::new(
                            "管理",
                            SystemTrayMenu::new()
                                .add_item(CustomMenuItem::new(
                                    "skills".to_string(),
                                    "打开 Skills 文件夹",
                                ))
                                .add_item(CustomMenuItem::new("settings".to_string(), "设置"))
                                .add_item(CustomMenuItem::new(
                                    "install_dir".to_string(),
                                    "打开安装目录",
                                ))
                                .add_item(CustomMenuItem::new("log".to_string(), "打开日志目录")),
                        ))
                        .add_native_item(SystemTrayMenuItem::Separator)
                        .add_item(CustomMenuItem::new("quit".to_string(), "退出 Harness")),
                ),
        )
        .on_system_tray_event(|app, event| {
            match event {
                SystemTrayEvent::LeftClick { .. } => {
                    if let Some(window) = app.get_window("main") {
                        let is_visible = window.is_visible().unwrap_or(false);
                        if is_visible {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                }
                SystemTrayEvent::MenuItemClick { id, .. } => {
                    match id.as_str() {
                        "open" => {
                            if let Some(window) = app.get_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "settings" => {
                            if let Err(e) = open_settings_inner(app) {
                                logger::log_to_file(
                                    &get_install_dir(),
                                    "ERROR",
                                    &format!("打开设置窗口失败: {}", e),
                                );
                            }
                        }
                        "skills" => {
                            if let Err(e) = open_skills_folder() {
                                logger::log_to_file(
                                    &get_install_dir(),
                                    "ERROR",
                                    &format!("打开 Skills 文件夹失败: {}", e),
                                );
                            }
                        }
                        "install_dir" => {
                            if let Err(e) = open_install_dir() {
                                logger::log_to_file(
                                    &get_install_dir(),
                                    "ERROR",
                                    &format!("打开安装目录失败: {}", e),
                                );
                            }
                        }
                        "log" => {
                            if let Err(e) = open_log_folder() {
                                logger::log_to_file(
                                    &get_install_dir(),
                                    "ERROR",
                                    &format!("打开日志目录失败: {}", e),
                                );
                            }
                        }
                        "quit" => {
                            // 退出前先杀掉服务进程
                            if let Some(state) = app.try_state::<ServiceManager>() {
                                state.kill_service_blocking();
                            }
                            let _ = app.exit(0);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        })
        .on_window_event(|event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
                let label = event.window().label().to_string();
                if label == "main" || label == "settings" {
                    api.prevent_close();
                    let _ = event.window().hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::load_config,
            commands::config::save_config,
            commands::service::start_harness_service,
            commands::service::stop_harness_service,
            commands::service::get_service_status,
            commands::service::check_nodejs,
            commands::update::check_for_updates,
            commands::dsh::check_dsh_version,
            commands::dsh::update_dsh,
            open_settings_window,
            open_log_folder,
            open_install_dir,
            open_skills_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 单实例保护：
/// - Windows：通过命名互斥体检测是否已有实例在运行；若有，把已有实例的主窗口
///   恢复到前台（而非静默退出），再让本实例退出。
/// - 其他平台：暂不限制（当前发布目标为 Windows）。
fn ensure_single_instance() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let name: Vec<u16> = "Local\\DeepseekHarness_SingleInstance"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // 句柄故意不关闭：进程存活期间保持互斥体存在，进程退出时由系统回收
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle == 0 {
            // 创建失败时保守处理：不阻止启动
            return true;
        }

        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // 已有实例在运行：把其主窗口恢复到前台，然后退出本实例（不静默）
            bring_existing_to_front();
            return false;
        }
    }

    true
}

/// 通过枚举顶层窗口，按标题精确匹配 "DeepseekHarness" 找到已运行实例的主窗口，
/// 并用 AttachThreadInput + SetForegroundWindow 稳定地将其恢复到前台（绕开系统对置顶的限制）。
#[cfg(target_os = "windows")]
fn bring_existing_to_front() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, SetForegroundWindow,
        ShowWindowAsync, SW_RESTORE,
    };

    struct FindCtx {
        target: Vec<u16>,
        found: isize,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: isize) -> i32 {
        let ctx = &mut *(lparam as *mut FindCtx);
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 && &buf[..len as usize] == ctx.target.as_slice() {
            ctx.found = hwnd as isize;
            0 // 命中即停止枚举
        } else {
            1
        }
    }

    let target: Vec<u16> = "DeepseekHarness".encode_utf16().collect();
    let mut ctx = FindCtx { target, found: 0 };
    unsafe {
        EnumWindows(Some(enum_cb), &mut ctx as *mut FindCtx as isize);
    }

    if ctx.found == 0 {
        return; // 没找到窗口，保守退出
    }

    unsafe {
        // 若窗口被最小化/隐藏，先恢复并展示
        ShowWindowAsync(ctx.found as HWND, SW_RESTORE);
        // 跨线程置顶：AttachThreadInput 可绕开系统对 SetForegroundWindow 的限制
        let curr = GetCurrentThreadId();
        let target_thread = GetWindowThreadProcessId(ctx.found as HWND, std::ptr::null_mut());
        if target_thread != 0 && target_thread != curr {
            let _ = AttachThreadInput(curr, target_thread, 1);
        }
        let _ = SetForegroundWindow(ctx.found as HWND);
        if target_thread != 0 && target_thread != curr {
            let _ = AttachThreadInput(curr, target_thread, 0);
        }
    }
}

fn open_settings_inner(app_handle: &tauri::AppHandle) -> tauri::Result<()> {
    let install_dir = get_install_dir();

    // 窗口已存在（可能是隐藏状态），直接显示
    if let Some(window) = app_handle.get_window("settings") {
        logger::log_to_file(&install_dir, "INFO", "设置窗口已存在，正在显示");
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }

    logger::log_to_file(&install_dir, "INFO", "正在创建设置窗口...");

    // 创建新设置窗口
    let _window = tauri::WindowBuilder::new(
        app_handle,
        "settings",
        tauri::WindowUrl::App("settings.html".into()),
    )
    .title("DeepseekHarness 设置")
    .inner_size(500.0, 450.0)
    .center()
    .decorations(true)
    .visible(true)
    .build()?;

    logger::log_to_file(&install_dir, "INFO", "设置窗口创建成功");
    Ok(())
}

#[tauri::command]
fn open_settings_window(app_handle: tauri::AppHandle) -> Result<(), String> {
    open_settings_inner(&app_handle).map_err(|e| format!("{}", e))
}

#[tauri::command]
fn open_log_folder() -> Result<(), String> {
    let install_dir = get_install_dir();
    let log_dir = install_dir.join("config").join("npmlog");
    std::fs::create_dir_all(&log_dir).map_err(|e| format!("{}", e))?;

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(&log_dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&log_dir).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&log_dir).spawn();
    }
    Ok(())
}

#[tauri::command]
fn open_install_dir() -> Result<(), String> {
    let install_dir = get_install_dir();

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(&install_dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&install_dir).spawn();
    }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&install_dir).spawn();
        }
        Ok(())
    }

    #[tauri::command]
    fn open_skills_folder() -> Result<(), String> {
        // dsh 默认 skills 目录：~/.agents/skills
        let home = home_dir().ok_or_else(|| "无法确定用户主目录".to_string())?;
        let skills_dir = home.join(".agents").join("skills");
        std::fs::create_dir_all(&skills_dir).map_err(|e| format!("创建 skills 目录失败: {}", e))?;

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer").arg(&skills_dir).spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&skills_dir).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&skills_dir).spawn();
        }
        Ok(())
    }
