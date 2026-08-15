use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

static APP_HANDLE: Mutex<Option<AppHandle>> = Mutex::new(None);

pub fn init(install_dir: &PathBuf) -> Result<(), String> {
    let log_dir = install_dir.join("config").join("applog");
    fs::create_dir_all(&log_dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
    Ok(())
}

pub fn set_app_handle(handle: AppHandle) {
    let mut guard = APP_HANDLE.lock().unwrap();
    *guard = Some(handle);
}

pub fn log_to_file(install_dir: &PathBuf, level: &str, message: &str) {
    let log_dir = install_dir.join("config").join("applog");
    let _ = fs::create_dir_all(&log_dir);
    let timestamp = chrono::Local::now().format("%H:%M:%S%.3f");
    let date_str = chrono::Local::now().format("%Y-%m-%d");
    let log_path = log_dir.join(format!("app-{}.log", date_str));

    let line = format!("[{}] [{}] {}\n", timestamp, level, message);

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = file.write_all(line.as_bytes());
    }

    let guard = APP_HANDLE.lock().unwrap();
    if let Some(handle) = guard.as_ref() {
        let _ = handle.emit_all("service-log", format!("[App] {}", message));
    }
}

pub fn cleanup_old_logs(install_dir: &PathBuf, days: i64) {
    let cutoff = chrono::Local::now() - chrono::Duration::days(days);
    cleanup_dir(&install_dir.join("config").join("applog"), cutoff);
    cleanup_dir(&install_dir.join("config").join("npmlog"), cutoff);
}

fn cleanup_dir(dir: &PathBuf, cutoff: chrono::DateTime<chrono::Local>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    if let Ok(modified) = metadata.modified() {
                        let modified_dt: chrono::DateTime<chrono::Local> = modified.into();
                        if modified_dt < cutoff {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }
}