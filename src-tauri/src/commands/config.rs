use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub port: u16,
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_theme() -> String {
    "system".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { port: 3080, theme: "system".to_string() }
    }
}

impl AppConfig {
    pub fn config_dir(app_dir: &PathBuf) -> PathBuf {
        app_dir.join("config")
    }

    pub fn config_path(app_dir: &PathBuf) -> PathBuf {
        Self::config_dir(app_dir).join("config.toml")
    }

    pub fn load(app_dir: &PathBuf) -> Result<Self, String> {
        let config_path = Self::config_path(app_dir);
        if !config_path.exists() {
            let config = Self::default();
            config.save(app_dir)?;
            return Ok(config);
        }
        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("读取配置失败: {}", e))?;
        match toml::from_str::<AppConfig>(&content) {
            Ok(config) => Ok(config),
            Err(_) => {
                let bak_path = config_path.with_extension("toml.bak");
                let _ = fs::copy(&config_path, &bak_path);
                let config = Self::default();
                config.save(app_dir)?;
                Ok(config)
            }
        }
    }

    pub fn save(&self, app_dir: &PathBuf) -> Result<(), String> {
        let config_dir = Self::config_dir(app_dir);
        fs::create_dir_all(&config_dir)
            .map_err(|e| format!("创建配置目录失败: {}", e))?;
        let config_path = Self::config_path(app_dir);
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("序列化配置失败: {}", e))?;
        fs::write(&config_path, content)
            .map_err(|e| format!("写入配置失败: {}", e))?;
        Ok(())
    }
}

#[tauri::command]
pub fn load_config() -> Result<AppConfig, String> {
    let install_dir = crate::commands::service::get_install_dir();
    AppConfig::load(&install_dir)
}

#[tauri::command]
pub fn save_config(config: AppConfig) -> Result<(), String> {
    let install_dir = crate::commands::service::get_install_dir();
    config.save(&install_dir)
}