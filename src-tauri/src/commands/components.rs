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
//! `recurbusybox` archive. It dispatches on `argv[0]`, so exposing the fifteen
//! tools is a matter of giving each enabled one a shim named after the tool
//! (see [`sync_component_shims`]) that points back at the one cached binary.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sea_orm::DatabaseConnection;

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
    /// `true` = cannot be disabled (RecurAgent's own tools).
    pub required: bool,
    /// Env vars applied when the component is run/installed.
    pub default_env: &'static [(&'static str, &'static str)],
}

pub const CATEGORY_BINARY: &str = "binary";
pub const CATEGORY_MCP: &str = "mcp";
pub const CATEGORY_SKILL: &str = "skill";

/// Pinned RecurBusyBox release. One version drives every platform asset and the
/// component list's `available_version`.
pub(crate) const RECURBUSYBOX_VERSION: &str = "0.1.5";
/// The multi-call executable inside every RecurBusyBox archive.
pub(crate) const RECURBUSYBOX_CMD: &str = "recurbusybox";
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
    ("windows-x86_64", "recurbusybox-windows-x86_64.zip"),
    ("linux-x86_64", "recurbusybox-linux-x86_64.tar.gz"),
    ("linux-aarch64", "recurbusybox-linux-aarch64.tar.gz"),
    ("darwin-x86_64", "recurbusybox-darwin-x86_64.tar.gz"),
    ("darwin-aarch64", "recurbusybox-darwin-aarch64.tar.gz"),
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

/// One RecurBusyBox tool entry, all served by the shared archive above.
macro_rules! busybox_entry {
    ($name:ident) => {
        ComponentDef {
            id: concat!("@", stringify!($name)),
            category: CATEGORY_BINARY,
            name: stringify!($name),
            description: concat!(
                stringify!($name),
                " — provided by the shared RecurBusyBox multi-call binary"
            ),
            tool: Some(stringify!($name)),
            required: false,
            default_env: &[],
        }
    };
}

/// The whole built-in registry. Ordering is stable and drives the settings
/// page's list order.
pub static COMPONENTS: &[ComponentDef] = &[
    // --- binary: RecurBusyBox (shared archive) ---------------------------
    busybox_entry!(rg),
    busybox_entry!(fd),
    busybox_entry!(sd),
    busybox_entry!(astgrep),
    busybox_entry!(jq),
    busybox_entry!(tokei),
    busybox_entry!(fzf),
    busybox_entry!(xsv),
    busybox_entry!(tree),
    busybox_entry!(extract),
    busybox_entry!(tar),
    busybox_entry!(zip),
    busybox_entry!(zcat),
    busybox_entry!(ps),
    busybox_entry!(dust),
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
        required: false,
        default_env: &[],
    },
    ComponentDef {
        id: "@ui-render-skill",
        category: CATEGORY_SKILL,
        name: "ui-render",
        description: "Bundled generative-UI skill pack kept up to date by RecurCode.",
        tool: None,
        required: false,
        default_env: &[],
    },
];

pub(crate) const COMPONENTS_UPDATED_EVENT: &str = "app://components-updated";

fn definition_for(id: &str) -> Option<&'static ComponentDef> {
    COMPONENTS.iter().find(|def| def.id == id)
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
    pub required: bool,
    pub enabled: bool,
    pub env: HashMap<String, String>,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
}

fn env_from_setting(model: Option<&crate::db::entities::component_setting::Model>) -> HashMap<String, String> {
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

pub(crate) async fn components_list_core(
    db: &AppDatabase,
) -> Result<Vec<ComponentInfo>, AppCommandError> {
    // The registry is the source of truth for which ids exist; make sure every
    // built-in has a row (enabled = true) before reading the joined view.
    let ids: Vec<&str> = COMPONENTS.iter().map(|def| def.id).collect();
    component_setting_service::ensure_defaults(&db.conn, &ids).await?;
    let settings = component_setting_service::list(&db.conn).await?;
    let by_id: HashMap<&str, &crate::db::entities::component_setting::Model> = settings
        .iter()
        .map(|m| (m.component_id.as_str(), m))
        .collect();

    let installed = installed_busybox_version();
    let available = RECURBUSYBOX_VERSION.to_string();

    Ok(COMPONENTS
        .iter()
        .map(|def| {
            let setting = by_id.get(def.id).copied();
            let is_binary = def.category == CATEGORY_BINARY;
            let (installed_version, available_version, update_available) = if is_binary {
                let update = installed
                    .as_deref()
                    .map(|v| v != available)
                    .unwrap_or(false);
                (installed.clone(), Some(available.clone()), update)
            } else {
                (None, None, false)
            };
            ComponentInfo {
                id: def.id.to_string(),
                category: def.category.to_string(),
                name: def.name.to_string(),
                description: def.description.to_string(),
                tool: def.tool.map(str::to_string),
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
    let def = definition_for(id)
        .ok_or_else(|| AppCommandError::not_found(format!("unknown component: {id}")))?;
    if def.required && !enabled {
        return Err(AppCommandError::invalid_input(format!(
            "{} is required by RecurAgent and cannot be disabled",
            def.name
        )));
    }
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
    definition_for(id)
        .ok_or_else(|| AppCommandError::not_found(format!("unknown component: {id}")))?;
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
    let ids: Vec<&str> = COMPONENTS.iter().map(|def| def.id).collect();
    component_setting_service::ensure_defaults(conn, &ids).await?;
    let settings = component_setting_service::list(conn).await?;
    let by_id: HashMap<&str, bool> = settings
        .iter()
        .map(|m| (m.component_id.as_str(), m.enabled))
        .collect();

    let source = installed_busybox_version()
        .and_then(|version| binary_cache::tool_binary_path(RECURBUSYBOX_TOOL_ID, &version, RECURBUSYBOX_CMD));

    let dir = binary_cache::components_bin_dir().map_err(|e| {
        AppCommandError::io_error("Failed to resolve components bin directory")
            .with_detail(e.to_string())
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        AppCommandError::io_error("Failed to create components bin directory")
            .with_detail(e.to_string())
    })?;

    for def in COMPONENTS.iter().filter(|d| d.category == CATEGORY_BINARY) {
        let Some(tool) = def.tool else { continue };
        let shim = dir.join(binary_cache::executable_file_name(tool));
        let enabled = by_id.get(def.id).copied().unwrap_or(true);
        if enabled {
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
