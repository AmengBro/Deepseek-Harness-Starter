use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub html_url: String,
    pub published_at: Option<String>,
}

const GITHUB_API: &str = "https://api.github.com/repos";
const REPO_OWNER: &str = "AmengBro";
const REPO_NAME: &str = "deepseek-harness";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tauri::command]
pub async fn check_for_updates(app_handle: AppHandle) -> Result<Option<ReleaseInfo>, String> {
    if REPO_OWNER == "your-github-username" {
        return Err(
            "自动更新尚未配置：请先在 src-tauri/src/commands/update.rs 中将 REPO_OWNER 改为你的 GitHub 用户名"
                .to_string(),
        );
    }

    let url = format!(
        "{}/{}/{}/releases/latest",
        GITHUB_API, REPO_OWNER, REPO_NAME
    );

    let client = reqwest::Client::builder()
        .user_agent("DeepseekHarness")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("请求 GitHub API 失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API 返回错误: {}", response.status()));
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

    let current = CURRENT_VERSION.to_string();
    let latest = release_version.clone();

    if version_greater(&latest, &current) {
        let info = ReleaseInfo {
            version: tag_name,
            html_url,
            published_at,
        };

        let _ = app_handle.emit_all("update-available", info.clone());
        Ok(Some(info))
    } else {
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
