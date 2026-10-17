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
//! `recurbusybox` archive (`rb`), which dispatches on `argv[0]`. It is resolved
//! and invoked at its fixed, RecurCode-owned path — never exposed as per-applet
//! names on `PATH`, so a host tool with the same name can neither shadow it nor
//! be shadowed by it.
//!
//! Applets are NOT independently configurable: the multi-call binary's switch
//! is the only switch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::acp::binary_cache;
use crate::acp::browser_tools::BrowserToolsRuntimeConfig;
use crate::acp::chat_authoring::ChatAuthoringRuntimeConfig;
use crate::acp::computer_tools::ComputerToolsRuntimeConfig;
use crate::acp::delegation::broker::DelegationBroker;
use crate::acp::delegation::listener::TokenRegistry;
use crate::acp::feedback::FeedbackRuntimeConfig;
use crate::acp::question::QuestionRuntimeConfig;
use crate::acp::registry;
use crate::acp::session_info::SessionInfoRuntimeConfig;
use crate::app_error::AppCommandError;
use crate::commands::generative_ui::{self, GenerativeUiComponentState};
use crate::commands::mcp_service::{
    rc_mcp_service_status_core, set_rc_mcp_tool_group_core, start_rc_mcp_service_core,
    RcMcpServiceStatus, RcMcpStatusSources, RcMcpToolGroupTargets,
};
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
    /// The bundled executable this component provides (binary category only).
    pub tool: Option<&'static str>,
    /// Applets a multi-call binary exposes (binary category only), listed in
    /// the detail pane for information only. They are not switched on their
    /// own: enabling the ONE executable in `tool` enables every applet, which
    /// `recurbusybox` selects from `argv[0]` when invoked at its fixed path.
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
        description: "RecurCode's collaboration MCP server for sub-agent delegation. \
            Versionless: it is built from this RecurCode release, so there is no separate download.",
        tool: None,
        tools: &[],
        required: false,
        default_env: &[],
    },
    ComponentDef {
        id: "@ui-render-skill",
        category: CATEGORY_SKILL,
        name: "ui-render",
        description: "Bundled generative-UI skill pack kept up to date by RecurCode. \
            Versioned by content hash, not by a release number.",
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
        if is_usable_sidecar(&candidate) {
            return Some(candidate);
        }
    }

    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        let candidate = dir.join(&filename);
        if is_usable_sidecar(&candidate) {
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
                .is_some_and(|n| is_staged_sidecar_name(&prefix, n));
            if staged && is_usable_sidecar(&path) {
                return Some(path);
            }
        }
    }
    None
}

/// Whether `file_name` is the staged name of the sidecar whose `prefix` is
/// `<name>-`: `<name>-<target-triple>` plus an optional platform executable
/// suffix.
///
/// The match is anchored to the CLOSED triple set, not a bare `starts_with`:
/// a prefix match also matched a DIFFERENT sidecar (`ra` matched
/// `ra-sandbox-<triple>`), and RecurCode then launched the wrong binary — a
/// 0-byte placeholder in the `ra-sandbox` case, which fails with `os error 193:
/// %1 is not a valid Win32 application`.
fn is_staged_sidecar_name(prefix: &str, file_name: &str) -> bool {
    let Some(rest) = file_name.strip_prefix(prefix) else {
        return false;
    };
    match rest.strip_suffix(".exe") {
        Some(triple) => STAGED_TARGET_TRIPLES.contains(&triple),
        // A Windows executable must carry its extension; a bare `-msvc` triple
        // is not the file `prepare-sidecars.mjs` stages (and matching it is how
        // the wrong `ra-sandbox` placeholder was launched).
        None => !rest.ends_with("-msvc") && STAGED_TARGET_TRIPLES.contains(&rest),
    }
}

/// A sidecar only counts when it is a real, non-empty file. `build.rs` writes a
/// zero-byte placeholder for any sidecar a plain `cargo check` did not stage,
/// and treating that as "found" made RecurCode launch a 0-byte executable
/// (`os error 193: %1 is not a valid Win32 application`) instead of reporting
/// the component as not installed.
fn is_usable_sidecar(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

/// The target triples `prepare-sidecars.mjs` stages sidecars for. Matching
/// against this closed set is what distinguishes `ra-<triple>` from an
/// unrelated `ra-sandbox-<triple>`.
const STAGED_TARGET_TRIPLES: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

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

/// The live machinery behind the two non-binary registry entries.
///
/// The Components tab is one row per component, while both real mechanisms are
/// multi-part: rc-mcp is a socket + binary + nine tool-group switches, and the
/// skill is a central copy plus one link per agent. These traits let the
/// registry code report and toggle the real thing while tests supply a fake,
/// without teaching the binary path anything about either mechanism.
#[async_trait::async_trait]
pub(crate) trait RcMcpComponent: Send + Sync {
    async fn status(&self) -> RcMcpServiceStatus;
    async fn set_enabled(
        &self,
        conn: &DatabaseConnection,
        emitter: &EventEmitter,
        enabled: bool,
    ) -> Result<(), AppCommandError>;
    async fn refresh(&self) -> Result<(), AppCommandError>;
}

#[async_trait::async_trait]
pub(crate) trait GenerativeUiComponent: Send + Sync {
    async fn state(&self) -> Result<GenerativeUiComponentState, AppCommandError>;
    async fn set_enabled(&self, enable: bool) -> Result<(), AppCommandError>;
    async fn refresh(&self, conn: &DatabaseConnection) -> Result<(), AppCommandError>;
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ComponentMachinery<'a> {
    pub mcp: Option<&'a dyn RcMcpComponent>,
    pub skill: Option<&'a dyn GenerativeUiComponent>,
}

/// The production implementation. It owns cheap clones of the same runtime
/// handles `AppState` holds, so both transports construct it the same way and
/// the `_core` helpers never see a Tauri type.
#[allow(dead_code)]
pub(crate) struct RuntimeMachinery {
    broker: Arc<DelegationBroker>,
    tokens: Arc<TokenRegistry>,
    feedback: FeedbackRuntimeConfig,
    question: QuestionRuntimeConfig,
    session_info: SessionInfoRuntimeConfig,
    authoring: ChatAuthoringRuntimeConfig,
    browser: BrowserToolsRuntimeConfig,
    computer: ComputerToolsRuntimeConfig,
}

impl RuntimeMachinery {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        broker: Arc<DelegationBroker>,
        tokens: Arc<TokenRegistry>,
        feedback: FeedbackRuntimeConfig,
        question: QuestionRuntimeConfig,
        session_info: SessionInfoRuntimeConfig,
        authoring: ChatAuthoringRuntimeConfig,
        browser: BrowserToolsRuntimeConfig,
        computer: ComputerToolsRuntimeConfig,
    ) -> Self {
        Self {
            broker,
            tokens,
            feedback,
            question,
            session_info,
            authoring,
            browser,
            computer,
        }
    }

    fn mcp_sources(&self) -> RcMcpStatusSources<'_> {
        RcMcpStatusSources {
            broker: &self.broker,
            tokens: &self.tokens,
            feedback: &self.feedback,
            question: &self.question,
            session_info: &self.session_info,
            authoring: &self.authoring,
            browser: &self.browser,
            computer: &self.computer,
        }
    }

    fn mcp_targets(&self) -> RcMcpToolGroupTargets<'_> {
        RcMcpToolGroupTargets {
            broker: &self.broker,
            feedback: &self.feedback,
            question: &self.question,
            session_info: &self.session_info,
            authoring: &self.authoring,
            browser: &self.browser,
            computer: &self.computer,
        }
    }
}

#[async_trait::async_trait]
impl RcMcpComponent for RuntimeMachinery {
    async fn status(&self) -> RcMcpServiceStatus {
        rc_mcp_service_status_core(self.mcp_sources()).await
    }

    async fn set_enabled(
        &self,
        conn: &DatabaseConnection,
        emitter: &EventEmitter,
        enabled: bool,
    ) -> Result<(), AppCommandError> {
        let status = self.status().await;
        if enabled {
            // On means "the service runs and at least one group is live". If
            // every group is off, turn the headline group (delegation) on;
            // otherwise respect the user's existing group choices.
            if !rc_mcp_component_enabled(&status) {
                set_rc_mcp_tool_group_core(
                    conn,
                    self.mcp_targets(),
                    emitter,
                    "delegation",
                    true,
                )
                .await?;
            }
            // The socket is the part this process can repair. A runtime with
            // no handle cannot bind one, but the group write above still took
            // effect, so report the toggle as done and log the rest.
            if let Err(err) = start_rc_mcp_service_core().await {
                tracing::warn!("[components] could not start rc-mcp broker: {err}");
            }
        } else {
            // Off means every tool group is off. Dependents go first so a
            // `browser_eval` write lands before the `browser` group that
            // gates it.
            let mut keys: Vec<String> = status
                .tool_groups
                .iter()
                .filter(|group| group.enabled)
                .map(|group| group.key.clone())
                .collect();
            keys.reverse();
            for key in keys {
                set_rc_mcp_tool_group_core(
                    conn,
                    self.mcp_targets(),
                    emitter,
                    &key,
                    false,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn refresh(&self) -> Result<(), AppCommandError> {
        let status = self.status().await;
        if status.binary_path.is_none() {
            return Err(AppCommandError::configuration_missing(
                "rc-mcp was not found beside RecurCode or on PATH; reinstall or set RC_MCP_BIN",
            ));
        }
        start_rc_mcp_service_core().await
    }
}

#[async_trait::async_trait]
impl GenerativeUiComponent for RuntimeMachinery {
    async fn state(&self) -> Result<GenerativeUiComponentState, AppCommandError> {
        generative_ui::component_state().await
    }

    async fn set_enabled(&self, enable: bool) -> Result<(), AppCommandError> {
        generative_ui::set_component_enabled(enable).await
    }

    async fn refresh(&self, conn: &DatabaseConnection) -> Result<(), AppCommandError> {
        generative_ui::refresh_component(conn).await
    }
}

/// The registry's `enabled` answer for rc-mcp: at least one real tool group is
/// live. Dependent switches (`browser_eval`) cannot answer this on their own.
fn rc_mcp_component_enabled(status: &RcMcpServiceStatus) -> bool {
    status
        .tool_groups
        .iter()
        .any(|group| group.requires.is_none() && group.enabled)
}

pub(crate) async fn components_list_core(
    db: &AppDatabase,
    machinery: ComponentMachinery<'_>,
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

    // Probe each non-binary mechanism once, then report what it actually says.
    // An absent source is an unknown mechanism, which must not read as enabled.
    let mcp_status = match machinery.mcp {
        Some(mcp) => Some(mcp.status().await),
        None => None,
    };
    let skill_state = match machinery.skill {
        Some(skill) => Some(skill.state().await?),
        None => None,
    };

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
            let stored_choice = setting.map(|m| m.enabled).unwrap_or(true);
            // Only RecurBusyBox is downloaded by this app; `ra` ships inside
            // the installer. `rb` therefore prefers the downloaded copy (which
            // may be newer) and otherwise reports the bundled one.
            let is_busybox = def.id == RECURBUSYBOX_TOOL_ID;
            let (enabled, installed_version, available_version, update_available) =
                if is_busybox {
                    let update = installed
                        .as_deref()
                        .map(|v| v != available)
                        .unwrap_or(false);
                    let shown = installed
                        .clone()
                        .or_else(|| busybox_bundled.then(|| available.clone()));
                    (stored_choice, shown, Some(available.clone()), update)
                } else if def.id == "@ra" {
                    (stored_choice, ra_bundled_version.clone(), None, false)
                } else if def.category == CATEGORY_MCP {
                    // No release number exists for a sidecar built from this
                    // same release; the description says so rather than a
                    // made-up version. `enabled` is the live group predicate,
                    // not the stored choice.
                    let enabled = mcp_status
                        .as_ref()
                        .is_some_and(rc_mcp_component_enabled);
                    (enabled, None, None, false)
                } else if def.category == CATEGORY_SKILL {
                    match skill_state.as_ref() {
                        Some(skill) => (
                            skill.enabled,
                            skill.installed_version.clone(),
                            skill.available_version.clone(),
                            skill.update_available,
                        ),
                        None => (false, None, None, false),
                    }
                } else {
                    (stored_choice, None, None, false)
                };
            // The persisted row records the user's CHOICE; the machinery is
            // the source of truth for the ACTUAL state. Keep the row, but say
            // when the two diverge instead of silently preferring either.
            if def.category != CATEGORY_BINARY && stored_choice != enabled {
                tracing::debug!(
                    "[components] {} choice {stored_choice} differs from live state {enabled}",
                    def.id
                );
            }
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
                enabled,
                env: env_from_setting(setting),
                installed_version,
                available_version,
                update_available,
            }
        })
        .collect())
}

/// Enable/disable a component. Refuses for `required` definitions — this is the
/// "RecurAgent's own tools cannot be disabled" rule. The change is persisted and
/// broadcast; binary components carry no PATH side effects.
pub(crate) async fn components_set_enabled_core(
    db: &AppDatabase,
    emitter: &EventEmitter,
    machinery: ComponentMachinery<'_>,
    id: &str,
    enabled: bool,
) -> Result<(), AppCommandError> {
    // Only a registry entry is switchable. An applet id (`@recurbusybox:rg`)
    // is not a component, so it is refused like any unknown id — enabling the
    // parent binary is what turns applets on.
    let def = definition_for(id)
        .ok_or_else(|| AppCommandError::not_found(format!("unknown component: {id}")))?;
    ensure_can_toggle(def, enabled)?;

    // Flip the real machinery FIRST: a failed toggle must not leave a stored
    // choice claiming it worked. Binary components need no pre-write action;
    // their switch is the shim sync below.
    match def.category {
        CATEGORY_MCP => machinery
            .mcp
            .ok_or_else(|| {
                AppCommandError::configuration_invalid(
                    "rc-mcp runtime state is not available in this process",
                )
            })?
            .set_enabled(&db.conn, emitter, enabled)
            .await?,
        CATEGORY_SKILL => machinery
            .skill
            .ok_or_else(|| {
                AppCommandError::configuration_invalid(
                    "generative-UI skill state is not available in this process",
                )
            })?
            .set_enabled(enabled)
            .await?,
        _ => {}
    }

    // The row remains the record of the user's choice; the machinery above is
    // what actually changed. A later read trusts the machinery over this row.
    component_setting_service::set_enabled(&db.conn, id, enabled).await?;
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

/// Download/refresh the shared RecurBusyBox archive. Progress rides the same
/// agent-install event stream the agent binary downloads use.
pub(crate) async fn components_update_core(
    db: &AppDatabase,
    emitter: &EventEmitter,
    machinery: ComponentMachinery<'_>,
    id: &str,
    task_id: &str,
) -> Result<(), AppCommandError> {
    let def = definition_for(id)
        .ok_or_else(|| AppCommandError::not_found(format!("unknown component: {id}")))?;

    // Neither non-binary entry has a release asset. Their honest "update" is a
    // refresh from what this RecurCode build already ships: rebind the broker
    // for rc-mcp, rewrite the central skill copy from the bundled content.
    match def.category {
        CATEGORY_MCP => {
            let result = machinery
                .mcp
                .ok_or_else(|| {
                    AppCommandError::configuration_invalid(
                        "rc-mcp runtime state is not available in this process",
                    )
                })?
                .refresh()
                .await;
            return finish_component_refresh(emitter, task_id, def, result);
        }
        CATEGORY_SKILL => {
            let result = machinery
                .skill
                .ok_or_else(|| {
                    AppCommandError::configuration_invalid(
                        "generative-UI skill state is not available in this process",
                    )
                })?
                .refresh(&db.conn)
                .await;
            return finish_component_refresh(emitter, task_id, def, result);
        }
        _ => {}
    }

    if def.category != CATEGORY_BINARY {
        return Err(AppCommandError::invalid_input(format!(
            "{} has no refresh path",
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

/// Report a non-binary refresh on the same install-event stream the binary
/// downloads use, so the settings toast does not need a second mechanism.
fn finish_component_refresh(
    emitter: &EventEmitter,
    task_id: &str,
    def: &ComponentDef,
    result: Result<(), AppCommandError>,
) -> Result<(), AppCommandError> {
    let (kind, message) = match &result {
        Ok(()) => (
            crate::commands::acp::AgentInstallEventKind::Completed,
            format!("{} refreshed", def.name),
        ),
        Err(err) => (
            crate::commands::acp::AgentInstallEventKind::Failed,
            err.to_string(),
        ),
    };
    crate::commands::acp::emit_agent_install_event(emitter, task_id, kind, message);
    if result.is_ok() {
        emit_components_updated(emitter);
    }
    result
}

fn emit_components_updated(emitter: &EventEmitter) {
    event_bridge::emit_event(emitter, COMPONENTS_UPDATED_EVENT, ());
}

// ---------------------------------------------------------------------------
// Tauri commands (desktop)
// ---------------------------------------------------------------------------

#[cfg(feature = "tauri-runtime")]
fn tauri_machinery(
    broker: &Arc<DelegationBroker>,
    tokens: &Arc<TokenRegistry>,
    feedback: &FeedbackRuntimeConfig,
    question: &QuestionRuntimeConfig,
    session_info: &SessionInfoRuntimeConfig,
    authoring: &ChatAuthoringRuntimeConfig,
    browser: &BrowserToolsRuntimeConfig,
    computer: &ComputerToolsRuntimeConfig,
) -> RuntimeMachinery {
    RuntimeMachinery::new(
        broker.clone(),
        tokens.clone(),
        feedback.clone(),
        question.clone(),
        session_info.clone(),
        authoring.clone(),
        browser.clone(),
        computer.clone(),
    )
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn components_list(
    db: tauri::State<'_, AppDatabase>,
    broker: tauri::State<'_, Arc<DelegationBroker>>,
    tokens: tauri::State<'_, Arc<TokenRegistry>>,
    feedback: tauri::State<'_, FeedbackRuntimeConfig>,
    question: tauri::State<'_, QuestionRuntimeConfig>,
    session_info: tauri::State<'_, SessionInfoRuntimeConfig>,
    authoring: tauri::State<'_, ChatAuthoringRuntimeConfig>,
    browser: tauri::State<'_, BrowserToolsRuntimeConfig>,
    computer: tauri::State<'_, ComputerToolsRuntimeConfig>,
) -> Result<Vec<ComponentInfo>, AppCommandError> {
    let machinery = tauri_machinery(
        broker.inner(),
        tokens.inner(),
        feedback.inner(),
        question.inner(),
        session_info.inner(),
        authoring.inner(),
        browser.inner(),
        computer.inner(),
    );
    components_list_core(
        &db,
        ComponentMachinery {
            mcp: Some(&machinery),
            skill: Some(&machinery),
        },
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn components_set_enabled(
    id: String,
    enabled: bool,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
    broker: tauri::State<'_, Arc<DelegationBroker>>,
    tokens: tauri::State<'_, Arc<TokenRegistry>>,
    feedback: tauri::State<'_, FeedbackRuntimeConfig>,
    question: tauri::State<'_, QuestionRuntimeConfig>,
    session_info: tauri::State<'_, SessionInfoRuntimeConfig>,
    authoring: tauri::State<'_, ChatAuthoringRuntimeConfig>,
    browser: tauri::State<'_, BrowserToolsRuntimeConfig>,
    computer: tauri::State<'_, ComputerToolsRuntimeConfig>,
) -> Result<(), AppCommandError> {
    let emitter = EventEmitter::Tauri(app);
    let machinery = tauri_machinery(
        broker.inner(),
        tokens.inner(),
        feedback.inner(),
        question.inner(),
        session_info.inner(),
        authoring.inner(),
        browser.inner(),
        computer.inner(),
    );
    components_set_enabled_core(
        &db,
        &emitter,
        ComponentMachinery {
            mcp: Some(&machinery),
            skill: Some(&machinery),
        },
        &id,
        enabled,
    )
    .await
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
#[allow(clippy::too_many_arguments)]
pub async fn components_update(
    id: String,
    task_id: String,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
    broker: tauri::State<'_, Arc<DelegationBroker>>,
    tokens: tauri::State<'_, Arc<TokenRegistry>>,
    feedback: tauri::State<'_, FeedbackRuntimeConfig>,
    question: tauri::State<'_, QuestionRuntimeConfig>,
    session_info: tauri::State<'_, SessionInfoRuntimeConfig>,
    authoring: tauri::State<'_, ChatAuthoringRuntimeConfig>,
    browser: tauri::State<'_, BrowserToolsRuntimeConfig>,
    computer: tauri::State<'_, ComputerToolsRuntimeConfig>,
) -> Result<(), AppCommandError> {
    let emitter = EventEmitter::Tauri(app);
    let machinery = tauri_machinery(
        broker.inner(),
        tokens.inner(),
        feedback.inner(),
        question.inner(),
        session_info.inner(),
        authoring.inner(),
        browser.inner(),
        computer.inner(),
    );
    components_update_core(
        &db,
        &emitter,
        ComponentMachinery {
            mcp: Some(&machinery),
            skill: Some(&machinery),
        },
        &id,
        &task_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::commands::mcp_service::RcMcpServiceState;
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

    /// In-memory stand-in for the rc-mcp machinery. `enabled` is the live
    /// group predicate the registry reads; the counters prove the core called
    /// the real toggle instead of only writing a settings row.
    #[derive(Default)]
    struct FakeMcp {
        enabled: AtomicBool,
        set_calls: AtomicBool,
        refresh_calls: AtomicBool,
    }

    impl FakeMcp {
        fn new(enabled: bool) -> Self {
            let fake = Self::default();
            fake.enabled.store(enabled, Ordering::SeqCst);
            fake
        }

        fn status(&self) -> RcMcpServiceStatus {
            let enabled = self.enabled.load(Ordering::SeqCst);
            RcMcpServiceStatus {
                state: if enabled {
                    RcMcpServiceState::Running
                } else {
                    RcMcpServiceState::Disabled
                },
                listening: true,
                socket_path: "test-socket".to_string(),
                binary_path: Some("rc-mcp".to_string()),
                tool_groups: vec![crate::commands::mcp_service::RcMcpToolGroup {
                    key: "delegation".to_string(),
                    enabled,
                    requires: None,
                }],
                companion_count: 0,
                session_count: 0,
                active_delegations: 0,
                depth_limit: 1,
                started_at: None,
                last_error: None,
                can_start: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl RcMcpComponent for FakeMcp {
        async fn status(&self) -> RcMcpServiceStatus {
            FakeMcp::status(self)
        }

        async fn set_enabled(
            &self,
            _conn: &DatabaseConnection,
            _emitter: &EventEmitter,
            enabled: bool,
        ) -> Result<(), AppCommandError> {
            self.enabled.store(enabled, Ordering::SeqCst);
            self.set_calls.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn refresh(&self) -> Result<(), AppCommandError> {
            self.refresh_calls.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    /// In-memory stand-in for the bundled skill: no filesystem, just the link
    /// aggregate and content hashes a real probe would return.
    struct FakeSkill {
        enabled: AtomicBool,
        update_available: AtomicBool,
        set_calls: AtomicBool,
        refresh_calls: AtomicBool,
    }

    impl FakeSkill {
        fn new(enabled: bool) -> Self {
            Self {
                enabled: AtomicBool::new(enabled),
                update_available: AtomicBool::new(false),
                set_calls: AtomicBool::new(false),
                refresh_calls: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl GenerativeUiComponent for FakeSkill {
        async fn state(&self) -> Result<GenerativeUiComponentState, AppCommandError> {
            Ok(GenerativeUiComponentState {
                enabled: self.enabled.load(Ordering::SeqCst),
                installed_version: Some("aabbccddeeff".to_string()),
                available_version: Some("aabbccddeeff".to_string()),
                update_available: self.update_available.load(Ordering::SeqCst),
                conflict: false,
            })
        }

        async fn set_enabled(&self, enable: bool) -> Result<(), AppCommandError> {
            self.enabled.store(enable, Ordering::SeqCst);
            self.set_calls.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn refresh(&self, _conn: &DatabaseConnection) -> Result<(), AppCommandError> {
            self.refresh_calls.store(true, Ordering::SeqCst);
            self.update_available.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    fn machinery<'a>(mcp: &'a FakeMcp, skill: &'a FakeSkill) -> ComponentMachinery<'a> {
        ComponentMachinery {
            mcp: Some(mcp),
            skill: Some(skill),
        }
    }

    async fn list(
        db: &AppDatabase,
        machinery: ComponentMachinery<'_>,
    ) -> Vec<ComponentInfo> {
        components_list_core(db, machinery).await.expect("list")
    }

    fn find<'a>(rows: &'a [ComponentInfo], id: &str) -> &'a ComponentInfo {
        rows.iter().find(|row| row.id == id).expect("row")
    }

    #[tokio::test]
    async fn binary_category_is_the_two_executables_not_their_applets() {
        let db = fresh_in_memory_db().await;
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(true);
        let list = list(&db, machinery(&mcp, &skill)).await;

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
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(true);
        let view = machinery(&mcp, &skill);

        assert!(
            components_set_enabled_core(&db, &emitter, view, "@ra", false)
                .await
                .is_err(),
            "RecurAgent is required"
        );

        // The applets are not individually switchable; the binary is the only
        // switch, and toggling it turns every shim on or off together.
        components_set_enabled_core(&db, &emitter, view, RECURBUSYBOX_TOOL_ID, false)
            .await
            .expect("the binary toggles");
        let rb = list(&db, view)
            .await
            .into_iter()
            .find(|c| c.id == RECURBUSYBOX_TOOL_ID)
            .unwrap();
        assert!(!rb.enabled);
        assert!(
            !mcp.set_calls.load(Ordering::SeqCst) && !skill.set_calls.load(Ordering::SeqCst),
            "the binary path never touches the non-binary machinery"
        );

        for unknown in ["@nope", "@recurbusybox:rg", "@recurbusybox:nope"] {
            assert!(
                components_set_enabled_core(&db, &emitter, view, unknown, false)
                    .await
                    .is_err(),
                "{unknown} is refused"
            );
        }
    }

    #[tokio::test]
    async fn env_round_trips_for_components() {
        let db = fresh_in_memory_db().await;
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(true);

        let mut env = HashMap::new();
        env.insert(
            "HTTP_PROXY".to_string(),
            "http://127.0.0.1:8080".to_string(),
        );
        components_set_env_core(&db, RECURBUSYBOX_TOOL_ID, env.clone())
            .await
            .expect("set env on the binary");

        let rb = list(&db, machinery(&mcp, &skill))
            .await
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

    #[tokio::test]
    async fn mcp_entry_reports_the_live_tool_group_state() {
        let db = fresh_in_memory_db().await;
        let emitter = EventEmitter::Noop;
        let mcp = FakeMcp::new(false);
        let skill = FakeSkill::new(true);
        let view = machinery(&mcp, &skill);

        // The machinery, not the persisted default (enabled=true), decides.
        let before = list(&db, view).await;
        let row = find(&before, "@rc-mcp");
        assert!(!row.enabled, "all groups off means the component is off");
        assert!(row.installed_version.is_none());
        assert!(row.available_version.is_none(), "no invented version");
        assert!(!row.update_available);

        components_set_enabled_core(&db, &emitter, view, "@rc-mcp", true)
            .await
            .expect("enable rc-mcp");
        assert!(mcp.set_calls.load(Ordering::SeqCst));
        assert!(mcp.enabled.load(Ordering::SeqCst));
        // The row records the user's choice; the live state matches it.
        let stored = component_setting_service::get(&db.conn, "@rc-mcp")
            .await
            .unwrap()
            .expect("choice row");
        assert!(stored.enabled);
        assert!(find(&list(&db, view).await, "@rc-mcp").enabled);

        components_update_core(&db, &emitter, view, "@rc-mcp", "task")
            .await
            .expect("refresh repairs the service");
        assert!(mcp.refresh_calls.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn skill_entry_reports_the_link_matrix() {
        let db = fresh_in_memory_db().await;
        let emitter = EventEmitter::Noop;
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(false);
        let view = machinery(&mcp, &skill);

        // Not linked into any agent yet: off, but a refresh would change the
        // central copy. There is no release download to offer.
        skill.update_available.store(true, Ordering::SeqCst);
        let before = list(&db, view).await;
        let row = find(&before, "@ui-render-skill");
        assert!(!row.enabled);
        assert_eq!(row.installed_version.as_deref(), Some("aabbccddeeff"));
        assert_eq!(row.available_version.as_deref(), Some("aabbccddeeff"));
        assert!(row.update_available);

        components_set_enabled_core(&db, &emitter, view, "@ui-render-skill", true)
            .await
            .expect("link the skill for every agent");
        assert!(skill.set_calls.load(Ordering::SeqCst));
        assert!(skill.enabled.load(Ordering::SeqCst));
        let stored = component_setting_service::get(&db.conn, "@ui-render-skill")
            .await
            .unwrap()
            .expect("choice row");
        assert!(stored.enabled);
        assert!(find(&list(&db, view).await, "@ui-render-skill").enabled);

        components_update_core(&db, &emitter, view, "@ui-render-skill", "task")
            .await
            .expect("refresh the bundled content");
        assert!(skill.refresh_calls.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn disabling_a_component_records_the_choice_and_reports_the_machinery() {
        let db = fresh_in_memory_db().await;
        let emitter = EventEmitter::Noop;
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(true);
        let view = machinery(&mcp, &skill);

        components_set_enabled_core(&db, &emitter, view, "@rc-mcp", false)
            .await
            .expect("disable rc-mcp");
        assert!(!mcp.enabled.load(Ordering::SeqCst));
        assert!(!find(&list(&db, view).await, "@rc-mcp").enabled);
        let stored = component_setting_service::get(&db.conn, "@rc-mcp")
            .await
            .unwrap()
            .expect("choice row");
        assert!(!stored.enabled);
    }

    #[test]
    fn rc_mcp_enabled_ignores_dependent_switches() {
        let mut status = FakeMcp::new(false).status();
        status.tool_groups.push(crate::commands::mcp_service::RcMcpToolGroup {
            key: "browser_eval".to_string(),
            enabled: true,
            requires: Some("browser".to_string()),
        });
        assert!(
            !rc_mcp_component_enabled(&status),
            "a dependent switch alone cannot make the component live"
        );
        status.tool_groups[0].enabled = true;
        assert!(rc_mcp_component_enabled(&status));
    }

    #[test]
    fn a_required_component_cannot_be_disabled() {
        let required = def("@keep", true);
        assert!(ensure_can_toggle(&required, false).is_err());
        assert!(ensure_can_toggle(&required, true).is_ok());
        assert!(ensure_can_toggle(&def("@free", false), false).is_ok());
    }

    #[test]
    fn a_staged_sidecar_name_matches_only_its_own_binary() {
        // The actual bug: `ra`'s prefix also matched `ra-sandbox`'s staged
        // file, so RecurCode launched the (0-byte) sandbox helper as the agent.
        assert!(is_staged_sidecar_name(
            "ra-",
            "ra-x86_64-pc-windows-msvc.exe"
        ));
        assert!(is_staged_sidecar_name("ra-", "ra-aarch64-apple-darwin"));
        assert!(!is_staged_sidecar_name(
            "ra-",
            "ra-sandbox-x86_64-pc-windows-msvc.exe"
        ));

        // The helper resolves under its OWN, longer name.
        assert!(is_staged_sidecar_name(
            "ra-sandbox-",
            "ra-sandbox-x86_64-pc-windows-msvc.exe"
        ));

        // A tool that is not a staged sidecar name at all is rejected.
        assert!(!is_staged_sidecar_name("ra-", "ra-x86_64-pc-windows-msvc"));
        assert!(!is_staged_sidecar_name("rb-", "rb-linux-x86_64.tar.gz"));
    }
}
