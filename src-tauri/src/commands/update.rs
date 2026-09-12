use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use crate::logger;
use crate::commands::service::get_install_dir;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub html_url: String,
    pub published_at: Option<String>,
}

/// 检查更新后的统一状态回报：无论「有更新 / 已最新 / 出错」都会通过 update-checked 事件推给前端，
/// 让前端能显示「GitHub 最新版本: vX.Y.Z」这一行，避免用户无感知。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateCheckResult {
    /// GitHub 上的最新 tag（含 v 前缀，如 "v1.2.0"）；出错时为 "unknown"
    pub latest: String,
    /// 当前本机版本（含 v 前缀）
    pub current: String,
    /// 是否需要更新（当前 < 最新）
    pub update_available: bool,
    /// 出错时的原因；成功时为 None
    pub error: Option<String>,
}

const GITHUB_API: &str = "https://api.github.com/repos";
const REPO_OWNER: &str = "AmengBro";
const REPO_NAME: &str = "Deepseek-Harness-Starter";

#[tauri::command]
pub async fn check_for_updates(app_handle: AppHandle) -> Result<Option<ReleaseInfo>, String> {
    // 当前版本（纯数字串，如 "1.2.1"，供比较）；显示用再拼 v 前缀
    let current_pure = app_handle.package_info().version.to_string();
    let current_display = format!("v{}", current_pure);

    // 统一回报状态（无论成功/已最新/出错都发一次，携带 latest 版本号）
    let report = |app_handle: &AppHandle, latest: String, update_available: bool, error: Option<String>| {
        let _ = app_handle.emit_all(
            "update-checked",
            UpdateCheckResult {
                latest,
                current: current_display.clone(),
                update_available,
                error,
            },
        );
    };

    if REPO_OWNER == "your-github-username" {
        let msg = "自动更新尚未配置：请先在 src-tauri/src/commands/update.rs 中将 REPO_OWNER 改为你的 GitHub 用户名".to_string();
        report(&app_handle, "unknown".to_string(), false, Some(msg.clone()));
        return Err(msg);
    }

    let url = format!(
        "{}/{}/{}/releases/latest",
        GITHUB_API, REPO_OWNER, REPO_NAME
    );

    let client = reqwest::Client::builder()
        .user_agent("DeepseekHarness")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| {
            let msg = format!("请求 GitHub API 失败: {}", e);
            logger::log_to_file(&get_install_dir(), "ERROR", &msg);
            report(&app_handle, "unknown".to_string(), false, Some(msg.clone()));
            msg
        })?;

    if !response.status().is_success() {
        let msg = format!("GitHub API 返回错误: {}", response.status());
        logger::log_to_file(&get_install_dir(), "ERROR", &msg);
        report(&app_handle, "unknown".to_string(), false, Some(msg.clone()));
        return Err(msg);
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析 GitHub 响应失败: {}", e))?;

    let tag_name = body["tag_name"]
        .as_str()
        .ok_or_else(|| "无法获取版本号".to_string())?
        .to_string();

    let html_url = body["html_url"]
        .as_str()
        .ok_or_else(|| "无法获取下载链接".to_string())?
        .to_string();

    let published_at = body["published_at"].as_str().map(|s| s.to_string());

    let release_version = tag_name.trim_start_matches('v').to_string();

    if version_greater(&release_version, &current_pure) {
        let info = ReleaseInfo {
            version: tag_name.clone(),
            html_url,
            published_at,
        };

        report(&app_handle, tag_name.clone(), true, None);
        let _ = app_handle.emit_all("update-available", info.clone());
        Ok(Some(info))
    } else {
        report(&app_handle, tag_name.clone(), false, None);
        Ok(None)
    }
}

fn version_greater(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    for (x, y) in va.iter().zip(vb.iter()) {
        if x > y {
            return true;
        }
        if x < y {
            return false;
        }
    }
    va.len() > vb.len()
}
