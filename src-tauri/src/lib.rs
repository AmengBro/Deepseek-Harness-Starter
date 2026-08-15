mod commands;
mod logger;

use commands::service::{get_install_dir, ServiceManager};
use commands::config::AppConfig;
use tauri::{
    CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu,
    SystemTrayMenuItem,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
                        .add_item(CustomMenuItem::new("settings".to_string(), "设置"))
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
            open_settings_window,
            open_log_folder,
            open_install_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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