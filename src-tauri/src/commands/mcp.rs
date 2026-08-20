// MCP 管理：读写 dsh 的 cordis.patch.yml（每个 MCP server 是一个 cordis 插件实例）
//
// dsh 没有原生 mcp CLI；MCP 通过 @deepseek-ai/dsh-mcp-client 插件接入。
// 配置位置：~/.dsh/profiles/<profile>/cordis.patch.yml（用户的 patch 层）
//
// 文档参考：@deepseek-ai/dsh-mcp-client/README.zh.md

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value as YamlValue};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use tauri::api::path::home_dir;

const MCP_PLUGIN_NAME: &str = "@deepseek-ai/dsh-mcp-client";

// ───────── 路径与 profile 探测 ─────────

/// dsh 主目录：默认 ~/.dsh，可被 `$DSH_HOME` 环境变量覆盖。
fn dsh_home() -> PathBuf {
    if let Ok(v) = env::var("DSH_HOME") {
        if !v.trim().is_empty() {
            return PathBuf::from(v.trim());
        }
    }
    home_dir().unwrap_or_else(|| PathBuf::from(".dsh"))
}

/// 探测 dsh 当前实际使用的 profile。
///
/// 启动器启动的是 `dsh web`（= `--profile web`），而用户也可能手动运行
/// `dsh` / `dsh headless` 等其它 profile。为避免写错目录导致“假添加”，
/// 这里动态选择真实生效的 profile，而非写死：
/// 1) 列出 `$DSH_HOME/profiles/` 下的子目录；
/// 2) 启动器主路径优先 `web`（含 `cordis.patch.yml` 时）；
/// 3) 否则返回任一已初始化（含 `cordis.patch.yml`）的 profile（用户手工运行的 headless 等）；
/// 4) 目录存在但未初始化时，优先 `web` > `headless`；
/// 5) 若 profiles 目录不存在/为空，回退 `web`（启动器启动 `dsh web` 会自动初始化它）。
fn get_active_profile() -> String {
    let profiles_dir = dsh_home().join("profiles");
    let mut with_patch: Option<String> = None;
    let mut all: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                all.push(name.clone());
                if path.join("cordis.patch.yml").exists() {
                    with_patch = Some(name);
                }
            }
        }
    }
    // 启动器主路径：优先 web profile（启动器运行 `dsh web`）
    if all.iter().any(|c| c == "web") && with_patch.as_deref() == Some("web") {
        return "web".to_string();
    }
    // 其次：任一已初始化（含 cordis.patch.yml）的 profile（用户手工运行的 headless 等）
    if let Some(p) = with_patch {
        return p;
    }
    // 再次：目录存在但未初始化时，优先 web > headless
    for preferred in ["web", "headless"] {
        if all.iter().any(|c| c == preferred) {
            return preferred.to_string();
        }
    }
    // 否则取第一个已知 profile
    if let Some(first) = all.into_iter().next() {
        return first;
    }
    // 回退：默认 web（启动器 `dsh web` 会自动初始化）
    "web".to_string()
}

fn get_mcp_config_path() -> Result<PathBuf, String> {
    let profile = get_active_profile();
    Ok(dsh_home().join("profiles").join(profile).join("cordis.patch.yml"))
}

fn ensure_dsh_dirs() -> Result<(), String> {
    let profile = get_active_profile();
    fs::create_dir_all(dsh_home().join("profiles").join(profile))
        .map_err(|e| format!("创建 profile 目录失败: {}", e))
}

fn read_patches() -> Result<Vec<YamlValue>, String> {
    ensure_dsh_dirs()?;
    let path = get_mcp_config_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {}", e))?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: YamlValue =
        serde_yaml::from_str(&content).map_err(|e| format!("解析 YAML 失败: {}", e))?;
    match value {
        YamlValue::Sequence(seq) => Ok(seq),
        YamlValue::Null => Ok(Vec::new()),
        _ => Err("cordis.patch.yml 根节点必须是数组".to_string()),
    }
}

fn write_patches(patches: &[YamlValue]) -> Result<(), String> {
    ensure_dsh_dirs()?;
    let path = get_mcp_config_path()?;
    if path.exists() {
        // 写前备份到 cordis.patch.yml.bak，保留最近一次原文件
        let bak = path.with_extension("yml.bak");
        let _ = fs::copy(&path, &bak);
    }
    let yaml = serde_yaml::to_string(patches).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(&path, yaml).map_err(|e| format!("写入失败: {}", e))
}

// ───────── 数据结构 ─────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpConfigPath {
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpServerEntry {
    pub id: String,
    pub server_name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

/// 前端提交的添加表单（id 可选，自动生成 `mcp-<serverName>`）
#[derive(Debug, Deserialize)]
pub struct McpServerInput {
    pub id: Option<String>,
    pub server_name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

// ───────── YAML 解析/构造 helper ─────────

fn yaml_str(v: &YamlValue, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(String::from)
}

fn yaml_str_arr(v: &YamlValue, k: &str) -> Option<Vec<String>> {
    v.get(k).and_then(|x| x.as_sequence()).map(|seq| {
        seq.iter().filter_map(|x| x.as_str().map(String::from)).collect()
    })
}

fn yaml_str_map(v: &YamlValue, k: &str) -> Option<HashMap<String, String>> {
    v.get(k).and_then(|x| x.as_mapping()).map(|m| {
        m.iter()
            .filter_map(|(k, x)| {
                let key = k.as_str()?.to_string();
                let val = x.as_str()?.to_string();
                Some((key, val))
            })
            .collect()
    })
}

fn parse_mcp_entry(id: &str, config: &YamlValue) -> Result<McpServerEntry, String> {
    Ok(McpServerEntry {
        id: id.to_string(),
        server_name: yaml_str(config, "serverName").unwrap_or_default(),
        transport: yaml_str(config, "transport").unwrap_or_else(|| "stdio".to_string()),
        command: yaml_str(config, "command"),
        args: yaml_str_arr(config, "args"),
        env: yaml_str_map(config, "env"),
        url: yaml_str(config, "url"),
        headers: yaml_str_map(config, "headers"),
    })
}

fn is_valid_server_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn build_patch_entry(id: &str, input: &McpServerInput) -> Result<YamlValue, String> {
    if !is_valid_server_name(&input.server_name) {
        return Err(format!(
            "serverName '{}' 不合法（仅 A-Za-z0-9_-，1-32 字符）",
            input.server_name
        ));
    }
    if input.transport != "stdio" && input.transport != "streamable-http" {
        return Err(format!(
            "transport '{}' 不支持（仅 stdio / streamable-http）",
            input.transport
        ));
    }

    let mut config = Mapping::new();
    config.insert(
        YamlValue::String("serverName".into()),
        YamlValue::String(input.server_name.clone()),
    );
    config.insert(
        YamlValue::String("transport".into()),
        YamlValue::String(input.transport.clone()),
    );

    if input.transport == "stdio" {
        let cmd = input
            .command
            .as_ref()
            .ok_or_else(|| "stdio 模式必须提供 command".to_string())?;
        config.insert(
            YamlValue::String("command".into()),
            YamlValue::String(cmd.clone()),
        );
        if let Some(args) = &input.args {
            let seq: Vec<YamlValue> = args.iter().map(|a| YamlValue::String(a.clone())).collect();
            config.insert(YamlValue::String("args".into()), YamlValue::Sequence(seq));
        }
        if let Some(env) = &input.env {
            let mut m = Mapping::new();
            for (k, v) in env {
                m.insert(YamlValue::String(k.clone()), YamlValue::String(v.clone()));
            }
            config.insert(YamlValue::String("env".into()), YamlValue::Mapping(m));
        }
    } else {
        let url = input
            .url
            .as_ref()
            .ok_or_else(|| "streamable-http 模式必须提供 url".to_string())?;
        config.insert(YamlValue::String("url".into()), YamlValue::String(url.clone()));
        if let Some(headers) = &input.headers {
            let mut m = Mapping::new();
            for (k, v) in headers {
                m.insert(YamlValue::String(k.clone()), YamlValue::String(v.clone()));
            }
            config.insert(YamlValue::String("headers".into()), YamlValue::Mapping(m));
        }
    }

    let mut entry = Mapping::new();
    entry.insert(
        YamlValue::String("id".into()),
        YamlValue::String(id.to_string()),
    );
    entry.insert(
        YamlValue::String("name".into()),
        YamlValue::String(MCP_PLUGIN_NAME.to_string()),
    );
    entry.insert(YamlValue::String("config".into()), YamlValue::Mapping(config));

    Ok(YamlValue::Mapping(entry))
}

// ───────── Tauri 命令 ─────────

#[tauri::command]
pub fn get_mcp_config_path_cmd() -> Result<McpConfigPath, String> {
    let path = get_mcp_config_path()?;
    Ok(McpConfigPath {
        path: path.to_string_lossy().to_string(),
        exists: path.exists(),
    })
}

#[tauri::command]
pub fn list_mcp_servers() -> Result<Vec<McpServerEntry>, String> {
    let patches = read_patches()?;
    let mut servers = Vec::new();
    for patch in patches {
        let name = patch.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name != MCP_PLUGIN_NAME {
            continue;
        }
        let id = patch.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let config = patch.get("config").cloned().unwrap_or(YamlValue::Null);
        servers.push(parse_mcp_entry(&id, &config)?);
    }
    Ok(servers)
}

#[tauri::command]
pub fn add_mcp_server(input: McpServerInput) -> Result<String, String> {
    let mut patches = read_patches()?;
    let id = input
        .id
        .clone()
        .unwrap_or_else(|| format!("mcp-{}", input.server_name));

    // 检查 id 和 serverName 冲突
    for patch in &patches {
        if let Some(existing) = patch.get("id").and_then(|v| v.as_str()) {
            if existing == id {
                return Err(format!("id '{}' 已存在", id));
            }
        }
        if patch.get("name").and_then(|v| v.as_str()) == Some(MCP_PLUGIN_NAME) {
            if let Some(cfg) = patch.get("config") {
                if let Some(sn) = cfg.get("serverName").and_then(|v| v.as_str()) {
                    if sn == input.server_name {
                        return Err(format!("serverName '{}' 已存在", input.server_name));
                    }
                }
            }
        }
    }

    let entry = build_patch_entry(&id, &input)?;
    patches.push(entry);
    write_patches(&patches)?;
    Ok(id)
}

#[tauri::command]
pub fn remove_mcp_server(id: String) -> Result<(), String> {
    let mut patches = read_patches()?;
    let before = patches.len();
    patches.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some(id.as_str()));
    if patches.len() == before {
        return Err(format!("未找到 id='{}' 的 MCP server", id));
    }
    write_patches(&patches)
}

#[tauri::command]
pub fn open_mcp_config_file() -> Result<(), String> {
    ensure_dsh_dirs()?;
    let path = get_mcp_config_path()?;
    if !path.exists() {
        fs::write(&path, "[]\n").map_err(|e| format!("创建配置文件失败: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        // explorer 选中文件
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let parent = path
            .parent()
            .ok_or_else(|| "无法获取父目录".to_string())?;
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
    Ok(())
}