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

/// Directory holding one JSON file per profile (`<root>/profiles/`).
fn profiles_dir(root: &Path) -> PathBuf {
    root.join("profiles")
}

/// Path of one profile's JSON file (`<root>/profiles/<id>.json`).
fn profile_path(root: &Path, id: &str) -> PathBuf {
    profiles_dir(root).join(format!("{id}.json"))
}

/// The active profile id: `RA_PROFILE` if set, else the `default-profile`
/// pointer file, else empty. The ONE resolution shared by the config
/// projection and the profile manager.
fn active_profile_id(root: &Path) -> String {
    std::env::var("RA_PROFILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            fs::read_to_string(root.join("default-profile"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default()
}

/// Whether Ra's own `config.json` names an LLM provider (or a model Ra can
/// infer one from) — the one thing `ra acp` insists on before it will start.
///
/// RecurCode preflights this so a first-run user is offered a friendly
/// "configure RecurAgent" prompt instead of the raw process-exit error the
/// ACP handshake would otherwise surface. Deliberately narrow: only `provider`
/// or `model` counts, mirroring the two fields `ra`'s factory consults
/// (`config.provider`, else `detect_provider(config.model)`). A `--provider`
/// flag or a `--profile` could satisfy `ra` without either field, but
/// RecurCode's launch passes neither, so with both absent the agent cannot
/// start.
pub(crate) fn ra_llm_configured() -> bool {
    ra_llm_configured_at(&config_path(&ra_data_root()))
}

fn ra_llm_configured_at(path: &Path) -> bool {
    let map = read_json_object_or_empty(path);
    let non_empty = |key: &str| {
        map.get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };
    non_empty("provider") || non_empty("model")
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

/// One profile with the fields the profiles manager renders. `provider` and
/// `model` come from the profile's nested `config` object; `data_dir` is the
/// top-level key. Unknown keys are preserved on disk but not surfaced here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaProfileDetail {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub data_dir: Option<String>,
    pub path: String,
}

/// Request to create a profile. `clone_from` names an existing profile id to
/// copy verbatim; when absent the new profile's `config` is seeded from the
/// root `config.json` ("whatever the app is using now").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaProfileCreate {
    pub name: String,
    #[serde(default)]
    pub clone_from: Option<String>,
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
    let profiles = list_profiles(&profiles_dir(root));
    let active_profile = active_profile_id(root);
    RaConfigProjection {
        config_path: path.display().to_string(),
        exists,
        model,
        mcp_servers,
        profiles,
        active_profile,
    }
}

/// Turn a slug of `name` into a file id: lowercase ASCII alphanumerics with
/// runs of anything else collapsed to a single `-`, leading/trailing `-`
/// trimmed. Non-ASCII names slug to the empty string and are rejected.
fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out
}

fn profile_detail_from_path(path: &Path, active: &str) -> RaProfileDetail {
    let map = read_json_object_or_empty(path);
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let name = map
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| id.clone());
    let config = map.get("config").and_then(|v| v.as_object());
    let provider = config
        .and_then(|c| c.get("provider"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let model = config
        .and_then(|c| c.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let data_dir = map
        .get("data_dir")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    RaProfileDetail {
        id: id.clone(),
        name,
        active: id == active,
        provider,
        model,
        data_dir,
        path: path.display().to_string(),
    }
}

/// Every profile file in `<root>/profiles/`, sorted by id, with `active`
/// flagged from the pointer file / `RA_PROFILE`.
pub fn list_profile_details() -> Vec<RaProfileDetail> {
    list_profile_details_at(&ra_data_root())
}

pub(crate) fn list_profile_details_at(root: &Path) -> Vec<RaProfileDetail> {
    let active = active_profile_id(root);
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(profiles_dir(root)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            out.push(profile_detail_from_path(&path, &active));
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Create a profile file. `clone_from` copies that profile's raw object
/// verbatim (preserving unknown keys) and overwrites identity fields;
/// otherwise `config` is seeded from the root `config.json`.
pub fn create_profile(req: RaProfileCreate) -> Result<RaProfileDetail, AcpError> {
    create_profile_at(&ra_data_root(), req)
}

pub(crate) fn create_profile_at(
    root: &Path,
    req: RaProfileCreate,
) -> Result<RaProfileDetail, AcpError> {
    let id = slugify(&req.name);
    if id.is_empty() {
        return Err(AcpError::protocol(
            "profile name must contain at least one letter or digit",
        ));
    }
    let path = profile_path(root, &id);
    if path.exists() {
        return Err(AcpError::protocol(format!(
            "a profile with id `{id}` already exists"
        )));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let mut map = match req.clone_from.as_deref().map(str::trim) {
        Some(from) if !from.is_empty() => {
            let src = profile_path(root, from);
            if !src.is_file() {
                return Err(AcpError::protocol(format!(
                    "cannot clone unknown profile `{from}`"
                )));
            }
            read_json_object_or_empty(&src)
        }
        _ => {
            let mut m = serde_json::Map::new();
            m.insert("enabled".into(), serde_json::Value::Bool(true));
            m.insert("data_dir".into(), serde_json::Value::Null);
            let root_config = read_json_object_or_empty(&config_path(root));
            m.insert("config".into(), serde_json::Value::Object(root_config));
            m
        }
    };
    map.insert("id".into(), serde_json::Value::String(id.clone()));
    map.insert(
        "name".into(),
        serde_json::Value::String(req.name.clone()),
    );
    map.insert("created_at".into(), serde_json::Value::String(now.clone()));
    map.insert("updated_at".into(), serde_json::Value::String(now));
    write_json_object_pretty(&path, &map)?;
    let active = active_profile_id(root);
    Ok(profile_detail_from_path(&path, &active))
}

/// Delete a profile file. The synthetic `_main` (RecurAgent's internal
/// `MAIN_PROFILE_ID`) and the currently active profile are protected.
pub fn delete_profile(id: &str) -> Result<(), AcpError> {
    delete_profile_at(&ra_data_root(), id)
}

pub(crate) fn delete_profile_at(root: &Path, id: &str) -> Result<(), AcpError> {
    if id == "_main" {
        return Err(AcpError::protocol(
            "the built-in `_main` profile cannot be deleted",
        ));
    }
    if active_profile_id(root) == id {
        return Err(AcpError::protocol(format!(
            "profile `{id}` is active; switch to another profile before deleting it"
        )));
    }
    let path = profile_path(root, id);
    if !path.is_file() {
        return Err(AcpError::protocol(format!("unknown profile `{id}`")));
    }
    fs::remove_file(&path)
        .map_err(|e| AcpError::protocol(format!("failed to delete {}: {e}", path.display())))?;
    Ok(())
}

/// Point `default-profile` at `id` for the next launch. Refuses an id with no
/// profile file so the pointer can never dangle.
pub fn set_active_profile(id: &str) -> Result<(), AcpError> {
    set_active_profile_at(&ra_data_root(), id)
}

pub(crate) fn set_active_profile_at(root: &Path, id: &str) -> Result<(), AcpError> {
    if !profile_path(root, id).is_file() {
        return Err(AcpError::protocol(format!("unknown profile `{id}`")));
    }
    let pointer = root.join("default-profile");
    fs::write(&pointer, id)
        .map_err(|e| AcpError::protocol(format!("failed to set active profile: {e}")))?;
    Ok(())
}

/// Apply a patch to Ra's native config, preserving every unknown key
/// Ra may store that this module does not model.
pub fn update_ra_config(update: RaConfigUpdate) -> Result<(), AcpError> {
    update_ra_config_at(&ra_data_root(), update)
}

/// Env-var name RecurCode stores a bound provider's key under in Ra's
/// `config.json` `env_vars` map (Ra's native credential mechanism).
const RA_PROVIDER_KEY_ENV: &str = "RC_MODEL_API_KEY";

/// Write a RecurCode-bound model provider into Ra's `~/.ra/config.json` as an
/// OpenAI-compatible endpoint: `provider`/`api_type` = `openai`, `base_url` =
/// the bound URL, and the key under `env_vars[RC_MODEL_API_KEY]` with
/// `api_key_env` pointing at it. Unknown keys are preserved. The Ra config
/// watcher hot-reloads the running runtime from this file, so a provider change
/// applies without a restart.
pub(crate) fn write_provider_to_ra_config(
    api_url: &str,
    api_key: &str,
) -> Result<(), AcpError> {
    write_provider_to_ra_config_at(&config_path(&ra_data_root()), api_url, api_key)
}

fn write_provider_to_ra_config_at(
    path: &Path,
    api_url: &str,
    api_key: &str,
) -> Result<(), AcpError> {
    let mut map = read_json_object_or_empty(path);
    map.insert("provider".into(), serde_json::Value::String("openai".into()));
    map.insert("api_type".into(), serde_json::Value::String("openai".into()));
    map.insert("api_key_env".into(), serde_json::Value::String(RA_PROVIDER_KEY_ENV.into()));
    if !api_url.trim().is_empty() {
        map.insert("base_url".into(), serde_json::Value::String(api_url.trim().into()));
    }
    if !api_key.trim().is_empty() {
        let env = map
            .entry("env_vars".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let obj = env.as_object_mut().ok_or_else(|| {
            AcpError::protocol("recur config `env_vars` must be an object")
        })?;
        obj.insert(
            RA_PROVIDER_KEY_ENV.to_string(),
            serde_json::Value::String(api_key.trim().to_string()),
        );
    }
    write_json_object_pretty(path, &map)
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

// ---------------------------------------------------------------------------
// Tauri commands (desktop)
// ---------------------------------------------------------------------------

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub fn ra_profile_list() -> Vec<RaProfileDetail> {
    list_profile_details()
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub fn ra_profile_create(req: RaProfileCreate) -> Result<RaProfileDetail, AcpError> {
    create_profile(req)
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub fn ra_profile_delete(id: String) -> Result<(), AcpError> {
    delete_profile(&id)
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub fn ra_profile_set_active(id: String) -> Result<(), AcpError> {
    set_active_profile(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn llm_configured_requires_provider_or_model() {
        // RecurCode's connect pre-check reads this to decide whether to block
        // with a "configure RecurAgent" prompt, so it must only say
        // "configured" for the two fields `ra acp` itself consults.
        let dir = std::env::temp_dir().join(format!("recur-cfg-llm-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = config_path(&dir);

        // No file at all → not configured.
        assert!(!ra_llm_configured_at(&path));

        // Base URL / key without a provider or model → still not configured.
        fs::write(
            &path,
            r#"{"base_url":"http://127.0.0.1:8188","api_key_env":"X"}"#,
        )
        .unwrap();
        assert!(!ra_llm_configured_at(&path));

        // A blank provider does not count.
        fs::write(&path, r#"{"provider":"   "}"#).unwrap();
        assert!(!ra_llm_configured_at(&path));

        // A model alone is enough — Ra infers the provider from it.
        fs::write(&path, r#"{"model":"claude-sonnet-4"}"#).unwrap();
        assert!(ra_llm_configured_at(&path));

        // So is an explicit provider.
        fs::write(&path, r#"{"provider":"openai"}"#).unwrap();
        assert!(ra_llm_configured_at(&path));

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

    #[test]
    fn write_provider_to_ra_config_sets_openai_endpoint_and_preserves_unknowns() {
        let dir = std::env::temp_dir().join(format!("recur-prov-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        fs::write(&path, r#"{"model":"gpt-4o","mcp_servers":[{"command":"x"}]}"#).unwrap();

        write_provider_to_ra_config_at(&path, "https://gw.example/v1", "sk-secret")
            .expect("write ok");

        let map = read_json_object_or_empty(&path);
        assert_eq!(map["provider"], json!("openai"));
        assert_eq!(map["api_type"], json!("openai"));
        assert_eq!(map["base_url"], json!("https://gw.example/v1"));
        assert_eq!(map["api_key_env"], json!("RC_MODEL_API_KEY"));
        assert_eq!(map["env_vars"]["RC_MODEL_API_KEY"], json!("sk-secret"));
        // Unknown / unrelated keys survive the write.
        assert_eq!(map["model"], json!("gpt-4o"));
        assert!(map["mcp_servers"].is_array());
        let _ = fs::remove_dir_all(&dir);
    }

    fn fresh_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(profiles_dir(&dir)).unwrap();
        dir
    }

    #[test]
    fn lists_profile_details_and_marks_active() {
        let dir = fresh_dir("recur-prof-list");
        fs::write(
            profile_path(&dir, "dev"),
            r#"{"id":"dev","name":"Dev","data_dir":"/d","config":{"provider":"openai","model":"gpt-4o"}}"#,
        )
        .unwrap();
        fs::write(profile_path(&dir, "prod"), r#"{"id":"prod","name":"Prod"}"#).unwrap();
        fs::write(dir.join("default-profile"), "prod").unwrap();

        let listed = list_profile_details_at(&dir);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "dev");
        assert_eq!(listed[0].name, "Dev");
        assert_eq!(listed[0].provider.as_deref(), Some("openai"));
        assert_eq!(listed[0].model.as_deref(), Some("gpt-4o"));
        assert_eq!(listed[0].data_dir.as_deref(), Some("/d"));
        assert!(!listed[0].active);
        assert!(listed[1].active, "default-profile wins");
        assert!(listed[1].path.ends_with("prod.json"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_profile_slugs_name_seeds_config_and_refuses_duplicates() {
        let dir = fresh_dir("recur-prof-create");
        fs::write(config_path(&dir), r#"{"provider":"anthropic","model":"claude"}"#).unwrap();

        let detail = create_profile_at(
            &dir,
            RaProfileCreate {
                name: "  Hello World! ".into(),
                clone_from: None,
            },
        )
        .expect("create");
        assert_eq!(detail.id, "hello-world");
        assert!(!detail.active);

        let raw = read_json_object_or_empty(&profile_path(&dir, "hello-world"));
        assert_eq!(raw["enabled"], json!(true));
        assert_eq!(raw["data_dir"], json!(null));
        assert_eq!(raw["config"]["provider"], json!("anthropic"));
        assert!(raw["created_at"].is_string());
        assert!(raw["updated_at"].is_string());

        // Same slug from a different spelling is a duplicate.
        assert!(create_profile_at(
            &dir,
            RaProfileCreate {
                name: "hello world".into(),
                clone_from: None,
            }
        )
        .is_err());
        // A name with no ASCII alphanumerics slugs to nothing.
        assert!(create_profile_at(
            &dir,
            RaProfileCreate {
                name: "---".into(),
                clone_from: None,
            }
        )
        .is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_profile_clones_existing_verbatim() {
        let dir = fresh_dir("recur-prof-clone");
        fs::write(
            profile_path(&dir, "src"),
            r#"{"id":"src","name":"Src","enabled":false,"data_dir":"/x","config":{"provider":"anthropic"},"custom":"kept"}"#,
        )
        .unwrap();

        let detail = create_profile_at(
            &dir,
            RaProfileCreate {
                name: "Copy".into(),
                clone_from: Some("src".into()),
            },
        )
        .expect("clone");
        assert_eq!(detail.id, "copy");
        let raw = read_json_object_or_empty(&profile_path(&dir, "copy"));
        assert_eq!(raw["enabled"], json!(false));
        assert_eq!(raw["data_dir"], json!("/x"));
        assert_eq!(raw["config"]["provider"], json!("anthropic"));
        assert_eq!(raw["custom"], json!("kept"), "unknown keys preserved");
        assert_eq!(raw["id"], json!("copy"));
        assert_eq!(raw["name"], json!("Copy"));

        assert!(create_profile_at(
            &dir,
            RaProfileCreate {
                name: "Nope".into(),
                clone_from: Some("missing".into()),
            }
        )
        .is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_refuses_main_and_active() {
        let dir = fresh_dir("recur-prof-delete");
        assert!(delete_profile_at(&dir, "_main").is_err(), "synthetic main");

        fs::write(profile_path(&dir, "active"), r#"{}"#).unwrap();
        fs::write(profile_path(&dir, "other"), r#"{}"#).unwrap();
        fs::write(dir.join("default-profile"), "active").unwrap();

        assert!(delete_profile_at(&dir, "active").is_err(), "active refused");
        assert!(delete_profile_at(&dir, "other").is_ok());
        assert!(!profile_path(&dir, "other").exists());
        assert!(delete_profile_at(&dir, "ghost").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_active_refuses_unknown() {
        let dir = fresh_dir("recur-prof-setactive");
        assert!(set_active_profile_at(&dir, "nope").is_err());

        fs::write(profile_path(&dir, "dev"), r#"{}"#).unwrap();
        set_active_profile_at(&dir, "dev").expect("set active");
        assert_eq!(active_profile_id(&dir), "dev");
        let _ = fs::remove_dir_all(&dir);
    }
}
