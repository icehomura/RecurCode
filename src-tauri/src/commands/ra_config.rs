//! Ra native config read/write.
//!
//! Ra (the `ra` CLI) is self-managed over ACP: it owns its model,
//! providers, MCP servers and skills in its own `config.json` + `profiles/`.
//! RecurCode's settings panel edits those native files so both the panel and
//! the running `ra acp` process read the SAME source of truth.
//!
//! RecurCode launches the agent with `--data-dir <ra_root>`, which pins
//! Ra's `config_home` to that directory, so the files live at
//! `<ra_root>/config.json` and `<ra_root>/profiles/<id>.json` — exactly
//! what this module reads/writes. `<ra_root>` defaults to `~/.ra`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::acp::error::AcpError;

/// Ra's data/config root. Kept in sync with the `--data-dir` the launch
/// path passes to `ra acp` and with the sessions root the `recur` parser reads.
pub fn ra_data_root() -> PathBuf {
    // RA_HOME overrides, else the brand state dir `~/.ra` (matches
    // `ra_core::brand::state_home`).
    if let Some(v) = std::env::var_os("RA_HOME") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    home_dir().join(".ra")
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn config_path(root: &Path) -> PathBuf {
    root.join("config.json")
}

/// Flat model-config keys a user edits from the panel. All optional so an
/// update can patch a subset. Serde names match Ra's `Config` exactly
/// (verbatim snake_case, no rename).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RaModelConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub api_type: Option<String>,
    pub context_window: Option<u32>,
    pub max_iterations: Option<u32>,
    pub format_after_edit: Option<bool>,
}

/// One MCP server entry, kept as an opaque JSON object so the panel round-trips
/// Ra's `McpServerConfig` shape (command/args/env/url/headers/oauth/...)
/// without RecurCode needing to model every field.
pub type RaMcpServer = serde_json::Map<String, serde_json::Value>;

/// A listed profile (id + display name).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaProfileSummary {
    pub id: String,
    pub name: String,
}

/// Everything the settings panel needs to render current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaConfigProjection {
    /// Absolute path of the `config.json` this projection reads, so the UI can
    /// show the operator where their edits land.
    pub config_path: String,
    pub exists: bool,
    pub model: RaModelConfig,
    /// Raw `mcp_servers` array, each entry an opaque object.
    pub mcp_servers: Vec<RaMcpServer>,
    pub profiles: Vec<RaProfileSummary>,
    /// Active profile id: `RA_PROFILE` if set, else the `default-profile`
    /// pointer, else empty (the built-in `coding` surface).
    pub active_profile: String,
}

/// A patch request. `None` fields leave the on-disk value untouched.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RaConfigUpdate {
    #[serde(flatten)]
    pub model: RaModelConfig,
    /// `Some(...)` replaces the whole array; `None` leaves it untouched.
    pub mcp_servers: Option<Vec<RaMcpServer>>,
    /// `Some(id)` writes the `default-profile` pointer for the next launch.
    pub active_profile: Option<String>,
}

fn read_json_object_or_empty(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        },
        Err(_) => serde_json::Map::new(),
    }
}

/// Atomically write a JSON object (tmp + rename), matching Ra's own
/// `Config::write_mutation` (which also preserves unknown keys by reading raw).
fn write_json_object_pretty(path: &Path, map: &serde_json::Map<String, serde_json::Value>) -> Result<(), AcpError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AcpError::protocol(format!("failed to create {}: {e}", parent.display())))?;
    }
    let value = serde_json::Value::Object(map.clone());
    let body = serde_json::to_string_pretty(&value)
        .map_err(|e| AcpError::protocol(format!("failed to serialize {}: {e}", path.display())))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body.as_bytes())
        .map_err(|e| AcpError::protocol(format!("failed to write {}: {e}", tmp.display())))?;
    fs::rename(&tmp, path)
        .map_err(|e| AcpError::protocol(format!("failed to rename onto {}: {e}", path.display())))?;
    Ok(())
}

fn opt_string(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn opt_u32(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<u32> {
    map.get(key).and_then(|v| v.as_u64()).map(|n| n as u32)
}

fn list_profiles(profiles_dir: &Path) -> Vec<RaProfileSummary> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(profiles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            // `name` may be absent on a minimal profile; fall back to the id.
            let name = read_json_object_or_empty(&path)
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| id.clone());
            out.push(RaProfileSummary { id, name });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Read Ra's native config into a projection. Never errors on absent or
/// malformed files (they yield an empty, "not configured yet" state).
pub fn load_ra_config() -> RaConfigProjection {
    load_ra_config_at(&ra_data_root())
}

pub(crate) fn load_ra_config_at(root: &Path) -> RaConfigProjection {
    let path = config_path(root);
    let exists = path.is_file();
    let map = read_json_object_or_empty(&path);
    let model = RaModelConfig {
        provider: opt_string(&map, "provider"),
        model: opt_string(&map, "model"),
        base_url: opt_string(&map, "base_url"),
        api_key_env: opt_string(&map, "api_key_env"),
        api_type: opt_string(&map, "api_type"),
        context_window: opt_u32(&map, "context_window"),
        max_iterations: opt_u32(&map, "max_iterations"),
        format_after_edit: map.get("format_after_edit").and_then(|v| v.as_bool()),
    };
    let mcp_servers = map
        .get("mcp_servers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_object().cloned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let profiles = list_profiles(&root.join("profiles"));
    let active_profile = std::env::var("RA_PROFILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            fs::read_to_string(root.join("default-profile"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    RaConfigProjection {
        config_path: path.display().to_string(),
        exists,
        model,
        mcp_servers,
        profiles,
        active_profile,
    }
}

/// Apply a patch to Ra's native config, preserving every unknown key
/// Ra may store that this module does not model.
pub fn update_ra_config(update: RaConfigUpdate) -> Result<(), AcpError> {
    update_ra_config_at(&ra_data_root(), update)
}

pub(crate) fn update_ra_config_at(root: &Path, update: RaConfigUpdate) -> Result<(), AcpError> {
    let path = config_path(root);
    let mut map = read_json_object_or_empty(&path);

    // Reject blank-but-present strings so a stray clear does not leave an empty
    // key Ra reads as a (broken) provider/model override.
    let put_str = |map: &mut serde_json::Map<String, serde_json::Value>, key: &str, v: &Option<String>| -> Result<(), AcpError> {
        match v.as_deref().map(str::trim) {
            Some("") if matches!(key, "provider" | "model") => {
                Err(AcpError::protocol(format!("recur {key} must not be empty")))
            }
            Some(s) => {
                map.insert(key.to_string(), serde_json::Value::String(s.to_string()));
                Ok(())
            }
            None => Ok(()),
        }
    };

    put_str(&mut map, "provider", &update.model.provider)?;
    put_str(&mut map, "model", &update.model.model)?;
    put_str(&mut map, "base_url", &update.model.base_url)?;
    put_str(&mut map, "api_key_env", &update.model.api_key_env)?;
    put_str(&mut map, "api_type", &update.model.api_type)?;
    if let Some(n) = update.model.context_window {
        map.insert("context_window".into(), serde_json::json!(n));
    }
    if let Some(n) = update.model.max_iterations {
        map.insert("max_iterations".into(), serde_json::json!(n));
    }
    if let Some(b) = update.model.format_after_edit {
        map.insert("format_after_edit".into(), serde_json::json!(b));
    }
    if let Some(servers) = update.mcp_servers {
        map.insert("mcp_servers".into(), serde_json::Value::Array(
            servers.into_iter().map(serde_json::Value::Object).collect(),
        ));
    }

    write_json_object_pretty(&path, &map)?;

    if let Some(id) = update.active_profile.as_deref().map(str::trim) {
        if !id.is_empty() {
            let pointer = root.join("default-profile");
            fs::write(&pointer, id)
                .map_err(|e| AcpError::protocol(format!("failed to set active profile: {e}")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("recur-cfg-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(
            config_path(&dir),
            r#"{"provider":"anthropic","model":"claude-sonnet-4","memory":{"max_inject_tokens":2500}}"#,
        )
        .unwrap();

        let update = RaConfigUpdate {
            model: RaModelConfig {
                model: Some("gpt-4o".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        update_ra_config_at(&dir, update).unwrap();

        // Model changed, unrelated keys survived.
        let after = load_ra_config_at(&dir);
        assert_eq!(after.model.model.as_deref(), Some("gpt-4o"));
        assert_eq!(after.model.provider.as_deref(), Some("anthropic"));
        let raw = read_json_object_or_empty(&config_path(&dir));
        assert!(raw.get("memory").is_some(), "unknown key dropped");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_provider_or_model_is_rejected() {
        let dir = std::env::temp_dir().join(format!("recur-cfg-err-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let update = RaConfigUpdate {
            model: RaModelConfig {
                provider: Some("  ".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(update_ra_config_at(&dir, update).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_deserializes_from_camel_case_wire() {
        // The frontend sends camelCase; flatten must inline the model fields and
        // rename mcpServers/activeProfile, matching `RaConfigUpdate`'s wire.
        let json = r#"{"provider":"anthropic","baseUrl":"https://x","maxIterations":0,"mcpServers":[{"command":"npx","args":[]}],"activeProfile":"dev"}"#;
        let upd: RaConfigUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(upd.model.provider.as_deref(), Some("anthropic"));
        assert_eq!(upd.model.base_url.as_deref(), Some("https://x"));
        assert_eq!(upd.model.max_iterations, Some(0));
        assert_eq!(upd.mcp_servers.as_ref().map(|v| v.len()), Some(1));
        assert_eq!(upd.active_profile.as_deref(), Some("dev"));
    }

    #[test]
    fn projection_serializes_to_camel_case() {
        let proj = RaConfigProjection {
            config_path: "/x/config.json".into(),
            exists: true,
            model: RaModelConfig {
                base_url: Some("https://y".into()),
                ..Default::default()
            },
            mcp_servers: vec![],
            profiles: vec![RaProfileSummary {
                id: "dev".into(),
                name: "Dev".into(),
            }],
            active_profile: "dev".into(),
        };
        let v = serde_json::to_value(&proj).unwrap();
        assert_eq!(v["configPath"].as_str(), Some("/x/config.json"));
        assert_eq!(v["model"]["baseUrl"].as_str(), Some("https://y"));
        assert_eq!(v["profiles"][0]["id"].as_str(), Some("dev"));
        assert_eq!(v["activeProfile"].as_str(), Some("dev"));
    }

    #[test]
    fn lists_profiles_from_dir() {
        let dir = std::env::temp_dir().join(format!("recur-cfg-prof-{}", std::process::id()));
        let prof = dir.join("profiles");
        fs::create_dir_all(&prof).unwrap();
        fs::write(prof.join("dev.json"), r#"{"id":"dev","name":"Dev"}"#).unwrap();
        fs::write(prof.join("prod.json"), r#"{"id":"prod"}"#).unwrap();
        let listed = list_profiles(&prof);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "dev");
        assert_eq!(listed[0].name, "Dev");
        assert_eq!(listed[1].name, "prod", "name falls back to id");
        let _ = fs::remove_dir_all(&dir);
    }
}
