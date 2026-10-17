//! Hardcoded registry of built-in "components" RecurCode ships and updates for
//! its agents, plus the commands that toggle/inspect them.
//!
//! The registry is intentionally compiled in and has NO create/delete API: a
//! component is something RecurCode owns, not something the user authors. The
//! only user-mutable state lives in `component_setting` (enabled / env), and
//! the `required` flag on a definition is the escape hatch that makes a
//! RecurAgent-owned component non-disableable.
//!
//! The `binary` category is served by ONE shared multi-call executable: the
//! `recurbusybox` archive. It dispatches on `argv[0]`, so enabling the binary
//! is a matter of giving every applet it provides a shim named after the tool
//! (see [`sync_component_shims`]) that points back at the one cached binary.
//!
//! Applets are NOT independently configurable: the multi-call binary's switch
//! is the only switch, so turning it off removes every applet shim at once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::acp::binary_cache;
use crate::acp::registry;
use crate::app_error::AppCommandError;
use crate::db::service::component_setting_service;
use crate::db::AppDatabase;
use crate::web::event_bridge::{self, EventEmitter};

/// One built-in component. All fields are `'static` because the whole registry
/// is a compile-time constant — see [`COMPONENTS`].
#[derive(Debug, Clone)]
pub struct ComponentDef {
    /// Tool-cache id. MUST start with `@` (see `binary_cache::is_tool_cache_id`)
    /// so it can never collide with an agent's registry id.
    pub id: &'static str,
    /// `"binary" | "mcp" | "skill"`.
    pub category: &'static str,
    /// Display name, e.g. `"rg"`.
    pub name: &'static str,
    /// One-line description shown in settings.
    pub description: &'static str,
    /// The executable this component provides on PATH (binary category only).
    pub tool: Option<&'static str>,
    /// Applets a multi-call binary exposes (binary category only), listed in
    /// the detail pane for information only. They are not switched on their
    /// own: enabling the ONE executable in `tool` writes a shim per applet,
    /// which is how `recurbusybox` picks its personality from `argv[0]`.
    /// Empty for single-purpose binaries (`ra`).
    pub tools: &'static [&'static str],
    /// `true` = cannot be disabled (the agent RecurCode depends on).
    pub required: bool,
    /// Env vars applied when the component is run/installed.
    pub default_env: &'static [(&'static str, &'static str)],
}

pub const CATEGORY_BINARY: &str = "binary";
pub const CATEGORY_MCP: &str = "mcp";
pub const CATEGORY_SKILL: &str = "skill";

/// Pinned RecurBusyBox release. One version drives every platform asset and the
/// component list's `available_version`.
pub(crate) const RECURBUSYBOX_VERSION: &str = "0.2.0";
/// The multi-call executable inside every RecurBusyBox archive.
pub(crate) const RECURBUSYBOX_CMD: &str = "rb";
/// Cache key for the ONE shared RecurBusyBox copy. Every binary component's
/// shim points at this single cached executable rather than duplicating the
/// archive per tool.
pub(crate) const RECURBUSYBOX_TOOL_ID: &str = "@recurbusybox";

const RECURBUSYBOX_RELEASE_BASE: &str =
    "https://github.com/icehomura/RecurBusyBox/releases/download";

/// Per-platform asset table for the shared archive. `(platform, asset)` uses
/// RecurCode's existing platform spelling (`registry::current_platform()`), so
/// the match in [`recur_busybox_archive_url`] is the only place platform
/// strings and asset names meet.
const RECURBUSYBOX_ASSETS: &[(&str, &str)] = &[
    ("windows-x86_64", "rb-windows-x86_64.zip"),
    ("linux-x86_64", "rb-linux-x86_64.tar.gz"),
    ("linux-aarch64", "rb-linux-aarch64.tar.gz"),
    ("darwin-x86_64", "rb-darwin-x86_64.tar.gz"),
    ("darwin-aarch64", "rb-darwin-aarch64.tar.gz"),
];

fn recur_busybox_archive_url() -> Option<String> {
    let platform = registry::current_platform();
    let asset = RECURBUSYBOX_ASSETS
        .iter()
        .find(|(p, _)| *p == platform)
        .map(|(_, asset)| *asset)?;
    Some(format!(
        "{RECURBUSYBOX_RELEASE_BASE}/v{RECURBUSYBOX_VERSION}/{asset}"
    ))
}

/// Sidecar checksum URL for the current platform's archive. Kept next to the
/// archive URL so a future install path can verify the download without
/// re-deriving the naming convention.
#[allow(dead_code)]
fn recur_busybox_sha256_url() -> Option<String> {
    recur_busybox_archive_url().map(|url| format!("{url}.sha256"))
}

/// The applets INSIDE the one `recurbusybox` binary. They are not binaries of
/// their own, so they are neither registry entries nor settings of their own:
/// enabling RecurBusyBox enables every applet, disabling it removes them all.
const RECURBUSYBOX_TOOLS: &[&str] = &[
    "rg", "fd", "sd", "astgrep", "jq", "tokei", "fzf", "xsv", "tree", "extract", "tar", "zip",
    "zcat", "ps", "dust",
];

/// The whole built-in registry. Ordering is stable and drives the settings
/// page's list order.
pub static COMPONENTS: &[ComponentDef] = &[
    // --- binary: the two executables RecurCode manages -------------------
    // RecurAgent is RecurCode's first-party agent; it is the one component that
    // may never be switched off, hence `required`. It ships inside RecurCode's
    // installer, so it has no self-update URL of its own today.
    ComponentDef {
        id: "@ra",
        category: CATEGORY_BINARY,
        name: "RecurAgent",
        description: "RecurCode's first-party coding agent.",
        tool: Some("ra"),
        tools: &[],
        required: true,
        default_env: &[],
    },
    // RecurBusyBox: ONE binary that dispatches to the toolset in
    // `RECURBUSYBOX_TOOLS` by `argv[0]`. The tools are listed in its detail
    // pane and toggled individually; the row itself governs the binary.
    ComponentDef {
        id: RECURBUSYBOX_TOOL_ID,
        category: CATEGORY_BINARY,
        name: "RecurBusyBox",
        description: "One multi-call binary providing the search, edit and text toolset.",
        tool: Some(RECURBUSYBOX_CMD),
        tools: RECURBUSYBOX_TOOLS,
        required: false,
        default_env: &[],
    },
    // --- mcp / skill placeholders ----------------------------------------
    // These are honest placeholders: they describe components RecurCode will
    // install, but the install path is NOT the binary-cache machinery below —
    // it will reuse the existing MCP-server and skill-pack plumbing. They are
    // listed so the three sub-tabs are not empty and the UI can render them.
    ComponentDef {
        id: "@rc-mcp",
        category: CATEGORY_MCP,
        name: "rc-mcp",
        description: "RecurCode's collaboration MCP server for sub-agent delegation.",
        tool: None,
        tools: &[],
        required: false,
        default_env: &[],
    },
    ComponentDef {
        id: "@ui-render-skill",
        category: CATEGORY_SKILL,
        name: "ui-render",
        description: "Bundled generative-UI skill pack kept up to date by RecurCode.",
        tool: None,
        tools: &[],
        required: false,
        default_env: &[],
    },
];

pub(crate) const COMPONENTS_UPDATED_EVENT: &str = "app://components-updated";

fn definition_for(id: &str) -> Option<&'static ComponentDef> {
    COMPONENTS.iter().find(|def| def.id == id)
}

/// Whether `def` may be flipped to `enabled`. Split out of the command so the
/// "a required component cannot be turned off" rule — the RecurAgent-style
/// keep-alive guarantee — is unit-testable without a registry entry that
/// happens to be required today.
fn ensure_can_toggle(def: &ComponentDef, enabled: bool) -> Result<(), AppCommandError> {
    if def.required && !enabled {
        return Err(AppCommandError::invalid_input(format!(
            "{} is required and cannot be disabled",
            def.name
        )));
    }
    Ok(())
}

/// One applet of a multi-call binary, as the settings detail pane renders it.
/// Purely informational: every applet follows its parent binary's switch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
}

/// Registry joined with persisted state, shaped for the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInfo {
    pub id: String,
    pub category: String,
    pub name: String,
    pub description: String,
    pub tool: Option<String>,
    /// Tools this binary dispatches to (empty for single-purpose binaries).
    #[serde(default)]
    pub tools: Vec<ToolInfo>,
    pub required: bool,
    pub enabled: bool,
    pub env: HashMap<String, String>,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
}

/// Every `component_setting.component_id` the registry implies — one per
/// registry entry. Applets are NOT settings: they ride their parent binary,
/// so they never get a row. Kept in ONE place so `ensure_defaults` and the
/// shim sync cannot drift apart.
fn all_setting_ids() -> Vec<String> {
    COMPONENTS.iter().map(|def| def.id.to_string()).collect()
}

fn env_from_setting(
    model: Option<&crate::db::entities::component_setting::Model>,
) -> HashMap<String, String> {
    model
        .and_then(|m| m.env_json.as_deref())
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default()
}

/// The best cached RecurBusyBox version, if any. `tool_installed_versions`
/// returns newest first, so the first entry is the one the shims should use.
fn installed_busybox_version() -> Option<String> {
    binary_cache::tool_installed_versions(RECURBUSYBOX_TOOL_ID, RECURBUSYBOX_CMD)
        .ok()
        .and_then(|versions| versions.into_iter().next())
}

/// Where RecurCode finds a binary it ships as a Tauri sidecar
/// (`bundle.externalBin`). An installed bundle puts the bare name beside the
/// app executable; `tauri dev` runs from `target/debug`, so the build tree's
/// `binaries/` directory — the one `prepare-sidecars.mjs` fills — is checked
/// too, under its `<name>-<target-triple>` staging name.
pub(crate) fn locate_sidecar(name: &str) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };

    let env_key = format!("RC_{}_BIN", name.to_uppercase());
    if let Some(raw) = std::env::var_os(&env_key) {
        let candidate = PathBuf::from(raw);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        let candidate = dir.join(&filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let prefix = format!("{name}-");
    if let Ok(entries) = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let staged = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix));
            if staged {
                return Some(path);
            }
        }
    }
    None
}

/// `<binary> --version` → its first version-looking token. The bundled `ra` is
/// whatever the rolling `latest` release held when RecurCode was built, so
/// asking the binary itself is the only honest source for its version.
fn probe_version(path: &Path) -> Option<String> {
    let out = std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()))
        .map(|token| token.trim_start_matches('v').to_string())
}

/// The `rb` to point shims at: the downloaded copy when one is installed, else
/// the sidecar RecurCode ships.
fn busybox_executable() -> Option<PathBuf> {
    installed_busybox_version()
        .and_then(|version| {
            binary_cache::tool_binary_path(RECURBUSYBOX_TOOL_ID, &version, RECURBUSYBOX_CMD)
        })
        .or_else(|| locate_sidecar(RECURBUSYBOX_CMD))
}

pub(crate) async fn components_list_core(
    db: &AppDatabase,
) -> Result<Vec<ComponentInfo>, AppCommandError> {
    // The registry is the source of truth for which ids exist; make sure every
    // built-in — and every tool of every multi-call binary — has a row
    // (enabled = true) before reading the joined view.
    let ids = all_setting_ids();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    component_setting_service::ensure_defaults(&db.conn, &refs).await?;
    let settings = component_setting_service::list(&db.conn).await?;
    let by_id: HashMap<&str, &crate::db::entities::component_setting::Model> = settings
        .iter()
        .map(|m| (m.component_id.as_str(), m))
        .collect();

    let installed = installed_busybox_version();
    let available = RECURBUSYBOX_VERSION.to_string();
    // The bundled sidecars let the settings page show a version before anything
    // is downloaded (`rb` uses the pinned release; `ra` is probed).
    let busybox_bundled = locate_sidecar(RECURBUSYBOX_CMD).is_some();
    let ra_bundled_version = locate_sidecar("ra").as_deref().and_then(probe_version);

    Ok(COMPONENTS
        .iter()
        .map(|def| {
            let setting = by_id.get(def.id).copied();
            // Only RecurBusyBox is downloaded by this app; `ra` ships inside
            // the installer. `rb` therefore prefers the downloaded copy (which
            // may be newer) and otherwise reports the bundled one.
            let is_busybox = def.id == RECURBUSYBOX_TOOL_ID;
            let (installed_version, available_version, update_available) = if is_busybox {
                let update = installed
                    .as_deref()
                    .map(|v| v != available)
                    .unwrap_or(false);
                let shown = installed
                    .clone()
                    .or_else(|| busybox_bundled.then(|| available.clone()));
                (shown, Some(available.clone()), update)
            } else if def.id == "@ra" {
                (ra_bundled_version.clone(), None, false)
            } else {
                (None, None, false)
            };
            let tools = def
                .tools
                .iter()
                .map(|tool| ToolInfo {
                    name: (*tool).to_string(),
                })
                .collect();
            ComponentInfo {
                id: def.id.to_string(),
                category: def.category.to_string(),
                name: def.name.to_string(),
                description: def.description.to_string(),
                tool: def.tool.map(str::to_string),
                tools,
                required: def.required,
                enabled: setting.map(|m| m.enabled).unwrap_or(true),
                env: env_from_setting(setting),
                installed_version,
                available_version,
                update_available,
            }
        })
        .collect())
}

/// Enable/disable a component. Refuses for `required` definitions — this is the
/// "RecurAgent's own tools cannot be disabled" rule. On success the agent PATH
/// shims are re-synced so the change takes effect on the next launch.
pub(crate) async fn components_set_enabled_core(
    db: &AppDatabase,
    emitter: &EventEmitter,
    id: &str,
    enabled: bool,
) -> Result<(), AppCommandError> {
    // Only a registry entry is switchable. An applet id (`@recurbusybox:rg`)
    // is not a component, so it is refused like any unknown id — enabling the
    // parent binary is what turns applets on.
    let def = definition_for(id)
        .ok_or_else(|| AppCommandError::not_found(format!("unknown component: {id}")))?;
    ensure_can_toggle(def, enabled)?;
    component_setting_service::set_enabled(&db.conn, id, enabled).await?;
    sync_component_shims(&db.conn).await?;
    emit_components_updated(emitter);
    Ok(())
}

/// Persist a component's env overrides. Rejects ids the registry does not know.
pub(crate) async fn components_set_env_core(
    db: &AppDatabase,
    id: &str,
    env: HashMap<String, String>,
) -> Result<(), AppCommandError> {
    if definition_for(id).is_none() {
        return Err(AppCommandError::not_found(format!(
            "unknown component: {id}"
        )));
    }
    let serialized = if env.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&env).map_err(|e| {
            AppCommandError::invalid_input("Failed to serialize component env")
                .with_detail(e.to_string())
        })?)
    };
    component_setting_service::set_env(&db.conn, id, serialized).await?;
    Ok(())
}

/// Download/refresh the shared RecurBusyBox archive and re-write every enabled
/// binary component's shim. Progress rides the same agent-install event stream
/// the agent binary downloads use.
pub(crate) async fn components_update_core(
    db: &AppDatabase,
    emitter: &EventEmitter,
    id: &str,
    task_id: &str,
) -> Result<(), AppCommandError> {
    let def = definition_for(id)
        .ok_or_else(|| AppCommandError::not_found(format!("unknown component: {id}")))?;
    if def.category != CATEGORY_BINARY {
        return Err(AppCommandError::invalid_input(format!(
            "{} is not a binary component and cannot be downloaded",
            def.name
        )));
    }
    // `ra` is shipped inside RecurCode's own installer, so there is no release
    // asset to fetch. Say so plainly instead of 404-ing on a URL that does not
    // exist (the RecurBusyBox release path below only applies to that binary).
    if def.id != RECURBUSYBOX_TOOL_ID {
        return Err(AppCommandError::invalid_input(format!(
            "{} is bundled with RecurCode and has no separate download",
            def.name
        )));
    }
    let url = recur_busybox_archive_url().ok_or_else(|| {
        AppCommandError::configuration_missing(format!(
            "RecurBusyBox is not available for platform {}",
            registry::current_platform()
        ))
    })?;

    crate::commands::acp::emit_agent_install_event(
        emitter,
        task_id,
        crate::commands::acp::AgentInstallEventKind::Started,
        "",
    );

    let emitter_clone = emitter.clone();
    let task_id_clone = task_id.to_string();
    let result = binary_cache::ensure_tool_binary_with_progress(
        RECURBUSYBOX_TOOL_ID,
        RECURBUSYBOX_VERSION,
        &url,
        RECURBUSYBOX_CMD,
        // The release publishes a `.sha256` sidecar; wiring it into the
        // download path is a follow-up. `None` keeps the install working today.
        None,
        move |msg| {
            crate::commands::acp::emit_agent_install_event(
                &emitter_clone,
                &task_id_clone,
                crate::commands::acp::AgentInstallEventKind::Log,
                msg,
            );
        },
    )
    .await;

    match result {
        Ok(_) => {
            sync_component_shims(&db.conn).await?;
            crate::commands::acp::emit_agent_install_event(
                emitter,
                task_id,
                crate::commands::acp::AgentInstallEventKind::Completed,
                format!("{} installed successfully", def.name),
            );
            emit_components_updated(emitter);
            Ok(())
        }
        Err(e) => {
            crate::commands::acp::emit_agent_install_event(
                emitter,
                task_id,
                crate::commands::acp::AgentInstallEventKind::Failed,
                e.to_string(),
            );
            Err(AppCommandError::task_execution_failed(e.to_string()))
        }
    }
}

fn emit_components_updated(emitter: &EventEmitter) {
    event_bridge::emit_event(emitter, COMPONENTS_UPDATED_EVENT, ());
}

/// Create/refresh one PATH shim per installed+enabled binary component and
/// remove shims for disabled ones.
///
/// The shim name is the whole mechanism: `recurbusybox` picks its personality
/// from `argv[0]`, so a hardlink/copy named `rg.exe` is what makes `rg` work.
/// On Windows symlinks need elevation or developer mode, so a hardlink (falling
/// back to a copy) is used; on unix a symlink is enough.
pub(crate) async fn sync_component_shims(conn: &DatabaseConnection) -> Result<(), AppCommandError> {
    let ids = all_setting_ids();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    component_setting_service::ensure_defaults(conn, &refs).await?;
    let settings = component_setting_service::list(conn).await?;
    let by_id: HashMap<&str, bool> = settings
        .iter()
        .map(|m| (m.component_id.as_str(), m.enabled))
        .collect();

    let source = busybox_executable();

    let dir = binary_cache::components_bin_dir().map_err(|e| {
        AppCommandError::io_error("Failed to resolve components bin directory")
            .with_detail(e.to_string())
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        AppCommandError::io_error("Failed to create components bin directory")
            .with_detail(e.to_string())
    })?;

    // RecurBusyBox is the one component whose applets become PATH shims. The
    // binary's switch is the only switch: on writes every applet shim, off
    // removes them all.
    let busybox_enabled = by_id.get(RECURBUSYBOX_TOOL_ID).copied().unwrap_or(true);
    for tool in RECURBUSYBOX_TOOLS {
        let shim = dir.join(binary_cache::executable_file_name(tool));
        if busybox_enabled {
            if let Some(source) = source.as_deref() {
                if let Err(e) = create_shim(source, &shim) {
                    tracing::warn!(
                        "[components] failed to create shim {} -> {}: {e}",
                        shim.display(),
                        source.display()
                    );
                }
            }
        } else if shim.exists() {
            if let Err(e) = std::fs::remove_file(&shim) {
                tracing::warn!("[components] failed to remove shim {}: {e}", shim.display());
            }
        }
    }
    Ok(())
}

fn create_shim(source: &Path, shim: &Path) -> std::io::Result<()> {
    // Always rebuild: the source may have moved to a new version directory
    // since the last sync, and a stale hardlink would keep dispatching to the
    // old binary.
    if shim.exists() {
        let _ = std::fs::remove_file(shim);
    }
    #[cfg(windows)]
    {
        // Hardlinks need no elevation and share the inode; a copy is the
        // fallback when the filesystem (e.g. some network mounts) refuses.
        if std::fs::hard_link(source, shim).is_err() {
            std::fs::copy(source, shim)?;
        }
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(source, shim)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands (desktop)
// ---------------------------------------------------------------------------

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn components_list(
    db: tauri::State<'_, AppDatabase>,
) -> Result<Vec<ComponentInfo>, AppCommandError> {
    components_list_core(&db).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn components_set_enabled(
    id: String,
    enabled: bool,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<(), AppCommandError> {
    let emitter = EventEmitter::Tauri(app);
    components_set_enabled_core(&db, &emitter, &id, enabled).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn components_set_env(
    id: String,
    env: HashMap<String, String>,
    db: tauri::State<'_, AppDatabase>,
) -> Result<(), AppCommandError> {
    components_set_env_core(&db, &id, env).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn components_update(
    id: String,
    task_id: String,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<(), AppCommandError> {
    let emitter = EventEmitter::Tauri(app);
    components_update_core(&db, &emitter, &id, &task_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::fresh_in_memory_db;

    /// A `ComponentDef` with every field spelled out, so a test can exercise
    /// branches (`required`) the shipped registry may not cover today.
    fn def(id: &'static str, required: bool) -> ComponentDef {
        ComponentDef {
            id,
            category: CATEGORY_BINARY,
            name: "x",
            description: "d",
            tool: Some("x"),
            tools: &[],
            required,
            default_env: &[],
        }
    }

    #[tokio::test]
    async fn binary_category_is_the_two_executables_not_their_applets() {
        let db = fresh_in_memory_db().await;
        let list = components_list_core(&db).await.expect("list");

        assert_eq!(list.len(), COMPONENTS.len(), "one row per registry entry");
        assert!(
            list.iter().all(|c| c.enabled),
            "built-ins default to enabled"
        );
        for category in [CATEGORY_BINARY, CATEGORY_MCP, CATEGORY_SKILL] {
            assert!(
                list.iter().any(|c| c.category == category),
                "category {category} has no components"
            );
        }

        let binaries: Vec<_> = list
            .iter()
            .filter(|c| c.category == CATEGORY_BINARY)
            .collect();
        let ids: Vec<&str> = binaries.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["@ra", "@recurbusybox"],
            "the binary tab lists the executables, not the 15 applets"
        );

        // `ra` ships inside RecurCode's installer and may never be switched off;
        // it therefore has no downloadable version of its own.
        let ra = binaries.iter().find(|c| c.id == "@ra").unwrap();
        assert!(ra.required, "RecurAgent cannot be disabled");
        assert!(
            ra.available_version.is_none(),
            "ra is bundled, not downloaded"
        );

        // `recurbusybox` is the one multi-call binary; its applets ride along.
        let rb = binaries
            .iter()
            .find(|c| c.id == RECURBUSYBOX_TOOL_ID)
            .unwrap();
        assert_eq!(rb.tools.len(), RECURBUSYBOX_TOOLS.len());
        assert_eq!(rb.available_version.as_deref(), Some(RECURBUSYBOX_VERSION));
        // The applets are listed for information, not switched individually.
        assert_eq!(
            rb.tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            RECURBUSYBOX_TOOLS
        );
    }

    #[tokio::test]
    async fn a_required_binary_cannot_be_disabled_and_applets_are_not_settings() {
        let db = fresh_in_memory_db().await;
        let emitter = EventEmitter::Noop;

        assert!(
            components_set_enabled_core(&db, &emitter, "@ra", false)
                .await
                .is_err(),
            "RecurAgent is required"
        );

        // The applets are not individually switchable; the binary is the only
        // switch, and toggling it turns every shim on or off together.
        components_set_enabled_core(&db, &emitter, RECURBUSYBOX_TOOL_ID, false)
            .await
            .expect("the binary toggles");
        let rb = components_list_core(&db)
            .await
            .expect("list")
            .into_iter()
            .find(|c| c.id == RECURBUSYBOX_TOOL_ID)
            .unwrap();
        assert!(!rb.enabled);

        for unknown in ["@nope", "@recurbusybox:rg", "@recurbusybox:nope"] {
            assert!(
                components_set_enabled_core(&db, &emitter, unknown, false)
                    .await
                    .is_err(),
                "{unknown} is refused"
            );
        }
    }

    #[tokio::test]
    async fn env_round_trips_for_components() {
        let db = fresh_in_memory_db().await;

        let mut env = HashMap::new();
        env.insert(
            "HTTP_PROXY".to_string(),
            "http://127.0.0.1:8080".to_string(),
        );
        components_set_env_core(&db, RECURBUSYBOX_TOOL_ID, env.clone())
            .await
            .expect("set env on the binary");

        let rb = components_list_core(&db)
            .await
            .expect("list")
            .into_iter()
            .find(|c| c.id == RECURBUSYBOX_TOOL_ID)
            .unwrap();
        assert_eq!(rb.env, env);

        // Applet ids are not settings, so they are refused like any unknown id.
        for unknown in ["@nope", "@recurbusybox:jq"] {
            assert!(components_set_env_core(&db, unknown, env.clone())
                .await
                .is_err());
        }
    }

    #[test]
    fn a_required_component_cannot_be_disabled() {
        let required = def("@keep", true);
        assert!(ensure_can_toggle(&required, false).is_err());
        assert!(ensure_can_toggle(&required, true).is_ok());
        assert!(ensure_can_toggle(&def("@free", false), false).is_ok());
    }
}
