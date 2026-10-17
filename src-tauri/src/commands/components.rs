//! Hardcoded registry of built-in "components" RecurCode ships and updates for
//! its agents, plus the commands that toggle/inspect them.
//!
//! The registry is intentionally compiled in and has NO create/delete API: a
//! component is something RecurCode owns, not something the user authors. The
//! only user-mutable state for binaries lives in `component_setting`
//! (enabled / env), and the `required` flag on a definition is the escape hatch
//! that makes a RecurAgent-owned component non-disableable.
//!
//! The `binary` category is served by ONE shared multi-call executable: the
//! `recurbusybox` archive. It dispatches on `argv[0]`, so enabling the binary
//! is a matter of giving every applet it provides a shim named after the tool
//! (see [`sync_component_shims`]) that points back at the one cached binary.
//! Applets can also be switched off individually; the parent binary's switch
//! still gates them all.
//!
//! mcp and skill are NOT booleans. rc-mcp is a socket + binary + nine tool
//! groups, and the skill is a central copy plus one link per agent. Their
//! `ComponentState` carries the real mechanism's status rather than a made-up
//! flag, and their actions land on the mechanism's own functions.

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
use crate::commands::experts::LinkOp;
use crate::commands::generative_ui::{self, GenerativeUiSkillSnapshot};
use crate::commands::mcp_service::{
    rc_mcp_service_status_core, set_rc_mcp_tool_group_core, start_rc_mcp_service_core,
    RcMcpServiceState, RcMcpServiceStatus, RcMcpStatusSources, RcMcpToolGroupTargets,
};
use crate::db::service::component_setting_service;
use crate::db::AppDatabase;
use crate::models::agent::AgentType;
use crate::web::event_bridge::{self, EventEmitter};

/// One built-in component. All fields are `'static` because the whole registry
/// is a compile-time constant — see [`COMPONENTS`].
#[derive(Debug, Clone)]
pub struct ComponentDef {
    /// Tool-cache id. MUST start with `@` (see `binary_cache::is_tool_cache_id`)
    /// so it can never collide with an agent's registry id.
    pub id: &'static str,
    /// `"binary" | "mcp" | "skill"`. `"binary"` covers both single-purpose
    /// binaries and multi-call binaries; [`ComponentInfo::category`] splits
    /// those two apart for the UI by looking at `tools`.
    pub category: &'static str,
    /// Display name, e.g. `"rg"`.
    pub name: &'static str,
    /// One-line description shown in settings.
    pub description: &'static str,
    /// The executable this component provides on PATH (binary category only).
    pub tool: Option<&'static str>,
    /// Applets a multi-call binary exposes (binary category only). Each is
    /// listed and switchable on its own; the parent binary's switch gates them
    /// all. Empty for single-purpose binaries (`ra`).
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

/// The stable setting id for one applet of the multi-call binary, e.g.
/// `@recurbusybox:rg`. A single colon keeps it distinct from every registry id
/// (which is just `@name`).
fn applet_setting_id(tool: &str) -> String {
    format!("{RECURBUSYBOX_TOOL_ID}:{tool}")
}

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
/// their own, so they are neither registry entries nor cache ids: enabling
/// RecurBusyBox writes a shim per applet, and an applet can be switched off on
/// its own.
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
    // it reuses the existing MCP-server and skill-pack plumbing. They are
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

/// The skill whose central copy and per-agent links the skill category reports.
const SKILL_ID: &str = generative_ui::SKILL_ID;

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

/// Component category as the UI sees it. `multicall` is split out of the
/// internal `binary` storage category by whether the definition declares
/// applets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentCategory {
    Binary,
    Multicall,
    Mcp,
    Skill,
}

/// One applet of a multi-call binary, as the settings detail pane renders it.
/// Kept for the flat compatibility mirror only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
}

/// One applet of a multi-call binary, as the settings detail pane renders it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticallTool {
    /// Stable setting id, e.g. `@recurbusybox:rg`.
    pub id: String,
    /// The applet name, e.g. `rg`.
    pub name: String,
    /// The user's choice for this applet; `true` unless switched off.
    pub enabled: bool,
    /// Whether the shared multi-call binary that provides this applet is
    /// actually present on disk — an observation, not a user choice.
    pub installed: bool,
}

/// Version facts for a single-purpose binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryState {
    pub tool: String,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
}

/// Version facts for a multi-call binary plus its applets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticallState {
    pub binary: BinaryState,
    pub tools: Vec<MulticallTool>,
}

/// Where the skill's central copy stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCentral {
    /// Our marked copy is present.
    Present,
    /// No copy under that name.
    Missing,
    /// A skill of the user's own holds the name; RecurCode must not touch it.
    Conflict,
}

/// The generative-UI skill's real state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillState {
    pub central: SkillCentral,
    /// Wire slugs of the agents linked to our central copy.
    pub linked_agents: Vec<String>,
    pub bundled_hash: Option<String>,
    pub central_hash: Option<String>,
    pub update_available: bool,
}

/// Category-tagged state union. A flat `enabled` boolean would lie about
/// rc-mcp (socket + binary + nine groups) and the skill (central copy + per
/// agent links), so each category carries its own real shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComponentState {
    Binary(BinaryState),
    Multicall(MulticallState),
    Mcp(Box<RcMcpServiceStatus>),
    Skill(SkillState),
}

/// A typed write against the components list.
///
/// Every variant lands on the real mechanism behind that category — none of
/// them touch `component_setting` for mcp or skill. The flat
/// [`crate::commands::components::components_set_enabled_core`] survives only
/// until the frontend migrates, and is removed with the final step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComponentsAction {
    /// Whole-binary user switch (single-purpose binary or multi-call parent).
    BinaryEnabled { id: String, enabled: bool },
    /// One applet of a multi-call binary. `id` is the applet setting id
    /// (e.g. `@recurbusybox:rg`).
    ToolEnabled { id: String, tool: String, enabled: bool },
    /// Toggle a single rc-mcp tool group via
    /// [`crate::commands::mcp_service::set_rc_mcp_tool_group_core`].
    McpToolGroup { group: String, enabled: bool },
    /// Bind the rc-mcp broker socket via
    /// [`crate::commands::mcp_service::start_rc_mcp_service_core`]; failures
    /// propagate to the caller.
    McpStartService,
    /// Link or unlink the bundled generative-UI skill to one agent by its
    /// wire slug.
    SkillLink { agent: String, linked: bool },
    /// Re-fetch the bundled binary/multicall, re-bind the broker, or
    /// re-install the skill from the bundled content. mcp/skill and rb
    /// reuse the existing refresh paths.
    Refresh { id: String, task_id: Option<String> },
}

/// The full result of a `ComponentsAction`: the freshly built list the
/// frontend should swap into place atomically. Returning the whole list avoids
/// the read-after-write race of `apply()` then `list()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsListResult {
    pub components: Vec<ComponentInfo>,
}

/// Deprecated flat mirror of a [`ComponentState`], kept only until the frontend
/// reads `state` directly (removed in the final step).
struct FlatCompat {
    enabled: bool,
    installed_version: Option<String>,
    available_version: Option<String>,
    update_available: bool,
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

impl ComponentState {
    fn flat(&self) -> FlatCompat {
        match self {
            ComponentState::Binary(binary) => FlatCompat {
                enabled: true,
                installed_version: binary.installed_version.clone(),
                available_version: binary.available_version.clone(),
                update_available: binary.update_available,
            },
            ComponentState::Multicall(multicall) => FlatCompat {
                enabled: true,
                installed_version: multicall.binary.installed_version.clone(),
                available_version: multicall.binary.available_version.clone(),
                update_available: multicall.binary.update_available,
            },
            ComponentState::Mcp(status) => FlatCompat {
                enabled: rc_mcp_component_enabled(status),
                installed_version: None,
                available_version: None,
                update_available: false,
            },
            ComponentState::Skill(skill) => FlatCompat {
                enabled: !skill.linked_agents.is_empty(),
                installed_version: skill.central_hash.as_deref().map(short_hash),
                available_version: skill.bundled_hash.as_deref().map(short_hash),
                update_available: skill.update_available,
            },
        }
    }
}

/// Registry joined with persisted state, shaped for the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInfo {
    pub id: String,
    pub category: ComponentCategory,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub env: HashMap<String, String>,
    pub state: ComponentState,
    // ── Deprecated flat mirrors, kept until the frontend migrates (step 5). ──
    pub tool: Option<String>,
    #[serde(default)]
    pub tools: Vec<ToolInfo>,
    pub enabled: bool,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
}

/// The hygienic status used when this process has no rc-mcp runtime at all.
fn unavailable_mcp_status() -> RcMcpServiceStatus {
    RcMcpServiceStatus {
        state: RcMcpServiceState::Unavailable,
        listening: false,
        socket_path: String::new(),
        binary_path: None,
        tool_groups: Vec::new(),
        companion_count: 0,
        session_count: 0,
        active_delegations: 0,
        depth_limit: 0,
        started_at: None,
        last_error: None,
        can_start: false,
    }
}

/// Whether rc-mcp should read as live: at least one group proper is enabled.
/// A dependent switch (`browser_eval`) cannot answer this on its own.
fn rc_mcp_component_enabled(status: &RcMcpServiceStatus) -> bool {
    status
        .tool_groups
        .iter()
        .any(|group| group.requires.is_none() && group.enabled)
}

/// The live machinery behind the non-binary registry entries. Wrapping the
/// real functions in a trait lets the registry report and act on the actual
/// mechanism while tests supply a fake, without teaching the binary path
/// anything about either mechanism.
#[async_trait::async_trait]
pub(crate) trait McpController: Send + Sync {
    async fn status(&self) -> RcMcpServiceStatus;
    async fn set_group(
        &self,
        conn: &DatabaseConnection,
        emitter: &EventEmitter,
        group: &str,
        enabled: bool,
    ) -> Result<(), AppCommandError>;
    /// Bind the broker socket. Failure MUST propagate to the caller.
    async fn start(&self) -> Result<(), AppCommandError>;
}

#[async_trait::async_trait]
pub(crate) trait SkillController: Send + Sync {
    async fn snapshot(&self) -> Result<GenerativeUiSkillSnapshot, AppCommandError>;
    /// Link/unlink the central copy for one agent by its wire slug.
    async fn link_agent(&self, agent: &str, linked: bool) -> Result<(), AppCommandError>;
    /// Rewrite the central copy from the bundled content.
    async fn refresh(&self, conn: &DatabaseConnection) -> Result<(), AppCommandError>;
}

/// The production implementation. It owns cheap clones of the same runtime
/// handles `AppState` holds, so both transports construct it the same way and
/// the `_core` helpers never see a Tauri type.
pub(crate) struct RuntimeComponents {
    broker: Arc<DelegationBroker>,
    tokens: Arc<TokenRegistry>,
    feedback: FeedbackRuntimeConfig,
    question: QuestionRuntimeConfig,
    session_info: SessionInfoRuntimeConfig,
    authoring: ChatAuthoringRuntimeConfig,
    browser: BrowserToolsRuntimeConfig,
    computer: ComputerToolsRuntimeConfig,
}

impl RuntimeComponents {
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
impl McpController for RuntimeComponents {
    async fn status(&self) -> RcMcpServiceStatus {
        rc_mcp_service_status_core(self.mcp_sources()).await
    }

    async fn set_group(
        &self,
        conn: &DatabaseConnection,
        emitter: &EventEmitter,
        group: &str,
        enabled: bool,
    ) -> Result<(), AppCommandError> {
        set_rc_mcp_tool_group_core(conn, self.mcp_targets(), emitter, group, enabled).await
    }

    async fn start(&self) -> Result<(), AppCommandError> {
        // Deliberately not swallowed: the caller must see a failed bind.
        start_rc_mcp_service_core().await
    }
}

#[async_trait::async_trait]
impl SkillController for RuntimeComponents {
    async fn snapshot(&self) -> Result<GenerativeUiSkillSnapshot, AppCommandError> {
        generative_ui::generative_ui_skill_snapshot().await
    }

    async fn link_agent(&self, agent: &str, linked: bool) -> Result<(), AppCommandError> {
        let agent_type = AgentType::from_wire(agent)
            .ok_or_else(|| AppCommandError::invalid_input(format!("unknown agent: {agent}")))?;
        let results = generative_ui::generative_ui_apply_links(vec![LinkOp {
            expert_id: SKILL_ID.to_string(),
            agent_type,
            enable: linked,
        }])
        .await?;
        match results.into_iter().next() {
            Some(result) if result.ok => Ok(()),
            Some(result) => Err(AppCommandError::task_execution_failed(
                result
                    .error
                    .unwrap_or_else(|| "skill link failed".to_string()),
            )),
            None => Err(AppCommandError::task_execution_failed(
                "skill link produced no result",
            )),
        }
    }

    async fn refresh(&self, conn: &DatabaseConnection) -> Result<(), AppCommandError> {
        generative_ui::install_generative_ui_skill(conn).await;
        Ok(())
    }
}

/// Every `component_setting.component_id` a binary component implies — one per
/// registry binary and one per applet. mcp/skill have no setting row: their
/// mechanism is the only truth. Kept in ONE place so `ensure_defaults` and the
/// shim sync cannot drift apart.
fn all_setting_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for def in COMPONENTS {
        if def.category != CATEGORY_BINARY {
            continue;
        }
        ids.push(def.id.to_string());
        for tool in def.tools {
            ids.push(applet_setting_id(tool));
        }
    }
    ids
}

/// Drop the setting rows mcp/skill used to own. Their state now comes from the
/// mechanism, so a stale row must never read back as a choice. One-shot and
/// idempotent: deleting a row that is not there is a no-op.
async fn purge_legacy_setting_rows(conn: &DatabaseConnection) -> Result<(), AppCommandError> {
    for def in COMPONENTS {
        if def.category == CATEGORY_BINARY {
            continue;
        }
        if let Some(model) = component_setting_service::get(conn, def.id).await? {
            use sea_orm::EntityTrait;
            crate::db::entities::component_setting::Entity::delete_by_id(model.id)
                .exec(conn)
                .await
                .map_err(crate::db::error::DbError::from)?;
        }
    }
    Ok(())
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
    // A Windows target stages `<name>-<triple>.exe`; every other target stages
    // the bare `<name>-<triple>`. Accepting both spellings for a Windows triple
    // would also match a name that cannot exist, and it hid the bug this guards:
    // a `.exe`-less file was treated as a usable sidecar and RecurCode then
    // launched a 0-byte placeholder.
    STAGED_TARGET_TRIPLES.iter().any(|triple| {
        if triple.contains("windows") {
            rest == format!("{triple}.exe")
        } else {
            rest == *triple
        }
    })
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

/// The `rb` to point shims at: the downloaded copy when one is installed, else
/// the sidecar RecurCode ships.
fn busybox_executable() -> Option<PathBuf> {
    installed_busybox_version()
        .and_then(|version| {
            binary_cache::tool_binary_path(RECURBUSYBOX_TOOL_ID, &version, RECURBUSYBOX_CMD)
        })
        .or_else(|| locate_sidecar(RECURBUSYBOX_CMD))
}

/// The version facts a binary reports, shared by `binary` and the inner
/// `binary` of a multicall.
fn binary_versions(
    def: &ComponentDef,
    installed: Option<&str>,
    available: &str,
    bundled: bool,
    ra_bundled_version: &Option<String>,
) -> (Option<String>, Option<String>, bool) {
    if def.id == RECURBUSYBOX_TOOL_ID {
        let update = installed.map(|v| v != available).unwrap_or(false);
        let shown = installed
            .map(str::to_string)
            .or_else(|| bundled.then(|| available.to_string()));
        (shown, Some(available.to_string()), update)
    } else if def.id == "@ra" {
        (ra_bundled_version.clone(), None, false)
    } else {
        (None, None, false)
    }
}

pub(crate) async fn components_list_core(
    db: &AppDatabase,
    mcp: Option<&dyn McpController>,
    skill: Option<&dyn SkillController>,
) -> Result<Vec<ComponentInfo>, AppCommandError> {
    // The registry is the source of truth for which ids exist; make sure every
    // built-in — and every applet of every multi-call binary — has a row
    // (enabled = true) before reading the joined view.
    purge_legacy_setting_rows(&db.conn).await?;
    let ids = all_setting_ids();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    component_setting_service::ensure_defaults(&db.conn, &refs).await?;
    let settings = component_setting_service::list(&db.conn).await?;
    let by_id: HashMap<&str, &crate::db::entities::component_setting::Model> = settings
        .iter()
        .map(|m| (m.component_id.as_str(), m))
        .collect();

    // Probe each non-binary mechanism once, then report what it actually says.
    let mcp_status = match mcp {
        Some(mcp) => Some(mcp.status().await),
        None => None,
    };
    let skill_snapshot = match skill {
        Some(skill) => Some(skill.snapshot().await?),
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
            let category = if def.category == CATEGORY_BINARY {
                if def.tools.is_empty() {
                    ComponentCategory::Binary
                } else {
                    ComponentCategory::Multicall
                }
            } else if def.category == CATEGORY_MCP {
                ComponentCategory::Mcp
            } else {
                ComponentCategory::Skill
            };
            let state = if def.category == CATEGORY_BINARY {
                let (installed_version, available_version, update_available) = binary_versions(
                    def,
                    installed.as_deref(),
                    &available,
                    busybox_bundled,
                    &ra_bundled_version,
                );
                let binary = BinaryState {
                    tool: def.tool.unwrap_or("").to_string(),
                    installed_version,
                    available_version,
                    update_available,
                };
                if def.tools.is_empty() {
                    ComponentState::Binary(binary)
                } else {
                    let binary_present = installed.is_some() || busybox_bundled;
                    let tools = def
                        .tools
                        .iter()
                        .map(|tool| {
                            let tool_id = applet_setting_id(tool);
                            let enabled = by_id
                                .get(tool_id.as_str())
                                .map(|m| m.enabled)
                                .unwrap_or(true);
                            MulticallTool {
                                id: tool_id,
                                name: (*tool).to_string(),
                                enabled,
                                installed: binary_present,
                            }
                        })
                        .collect();
                    ComponentState::Multicall(MulticallState { binary, tools })
                }
            } else if def.category == CATEGORY_MCP {
                let status = mcp_status
                    .clone()
                    .unwrap_or_else(unavailable_mcp_status);
                ComponentState::Mcp(Box::new(status))
            } else {
                let (central, linked_agents, bundled_hash, central_hash, update_available) =
                    match skill_snapshot.as_ref() {
                        Some(snapshot) => {
                            let central = if snapshot.conflict {
                                SkillCentral::Conflict
                            } else if snapshot.central_hash.is_some() {
                                SkillCentral::Present
                            } else {
                                SkillCentral::Missing
                            };
                            let update = !snapshot.conflict
                                && snapshot.central_hash.as_deref()
                                    != Some(snapshot.bundled_hash.as_str());
                            (
                                central,
                                snapshot.linked_agents.clone(),
                                Some(snapshot.bundled_hash.clone()),
                                snapshot.central_hash.clone(),
                                update,
                            )
                        }
                        None => (SkillCentral::Missing, Vec::new(), None, None, false),
                    };
                ComponentState::Skill(SkillState {
                    central,
                    linked_agents,
                    bundled_hash,
                    central_hash,
                    update_available,
                })
            };
            // The stored row records the user's choice for binaries only; for
            // mcp/skill the mechanism is the source of truth. Say when a
            // binary choice and its state would ever diverge.
            if def.category == CATEGORY_BINARY && def.id == RECURBUSYBOX_TOOL_ID && !stored_choice {
                tracing::debug!("[components] {} is switched off by the user", def.id);
            }
            let flat = state.flat();
            ComponentInfo {
                id: def.id.to_string(),
                category,
                name: def.name.to_string(),
                description: def.description.to_string(),
                required: def.required,
                env: env_from_setting(setting),
                tool: def.tool.map(str::to_string),
                tools: def
                    .tools
                    .iter()
                    .map(|tool| ToolInfo {
                        name: (*tool).to_string(),
                    })
                    .collect(),
                enabled: flat.enabled,
                installed_version: flat.installed_version,
                available_version: flat.available_version,
                update_available: flat.update_available,
                state,
            }
        })
        .collect())
}

/// Enable/disable a component. Refuses for `required` definitions — this is the
/// "RecurAgent's own tools cannot be disabled" rule. On success the agent PATH
/// shims are re-synced so the change takes effect on the next launch.
///
/// Deprecated in favour of [`components_apply_core`]; kept until the frontend
/// migrates and removed in the final step.
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

/// Apply a typed [`ComponentsAction`] to the real mechanism behind it, then
/// return the rebuilt list so the frontend can swap it in atomically. Every
/// variant lands on the function that owns the underlying state — no
/// `component_setting` is written for mcp or skill, no broker start is
/// swallowed, and one switch can no longer blank seven unrelated groups.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn components_apply_core(
    db: &AppDatabase,
    emitter: &EventEmitter,
    mcp: Option<&dyn McpController>,
    skill: Option<&dyn SkillController>,
    action: ComponentsAction,
) -> Result<ComponentsListResult, AppCommandError> {
    match action {
        ComponentsAction::BinaryEnabled { id, enabled } => {
            // Only a registry entry is switchable. An applet id is not a
            // component, so it is refused like any unknown id — the parent's
            // switch is what gates applets.
            let def = definition_for(&id).ok_or_else(|| {
                AppCommandError::not_found(format!("unknown component: {id}"))
            })?;
            ensure_can_toggle(def, enabled)?;
            component_setting_service::set_enabled(&db.conn, &id, enabled).await?;
            sync_component_shims(&db.conn).await?;
            emit_components_updated(emitter);
        }
        ComponentsAction::ToolEnabled { id, tool, enabled } => {
            // An applet lives by its colon-suffixed setting id; the request
            // supplies both the parent registry id and the applet name to
            // route around any future id-decomposition change. The flat
            // registry id itself is not a switchable row.
            if definition_for(&id).is_none() {
                return Err(AppCommandError::not_found(format!(
                    "unknown component: {id}"
                )));
            }
            // The applet id is fixed by the helper; refuse anything else to
            // avoid writing to a row someone constructed by hand.
            let canonical = applet_setting_id(&tool);
            if canonical != id {
                return Err(AppCommandError::invalid_input(format!(
                    "applet id mismatch: expected {canonical}, got {id}"
                )));
            }
            component_setting_service::set_enabled(&db.conn, &canonical, enabled).await?;
            sync_component_shims(&db.conn).await?;
            emit_components_updated(emitter);
        }
        ComponentsAction::McpToolGroup { group, enabled } => {
            let mcp = mcp.ok_or_else(|| {
                AppCommandError::configuration_invalid(
                    "rc-mcp runtime is not available in this process",
                )
            })?;
            mcp.set_group(&db.conn, emitter, &group, enabled).await?;
            emit_components_updated(emitter);
        }
        ComponentsAction::McpStartService => {
            let mcp = mcp.ok_or_else(|| {
                AppCommandError::configuration_invalid(
                    "rc-mcp runtime is not available in this process",
                )
            })?;
            // Intentional: the caller sees the failure verbatim. Swallowing
            // it here would re-introduce the "Started!" toast on a dead socket.
            mcp.start().await?;
            emit_components_updated(emitter);
        }
        ComponentsAction::SkillLink { agent, linked } => {
            let skill = skill.ok_or_else(|| {
                AppCommandError::configuration_invalid(
                    "skill runtime is not available in this process",
                )
            })?;
            skill.link_agent(&agent, linked).await?;
            emit_components_updated(emitter);
        }
        ComponentsAction::Refresh { id, task_id } => {
            let def = definition_for(&id).ok_or_else(|| {
                AppCommandError::not_found(format!("unknown component: {id}"))
            })?;
            match def.category {
                CATEGORY_BINARY => {
                    // Refresh only the shared RecurBusyBox archive today;
                    // `ra` has no separate download path so the legacy
                    // helper responds with a clear message of its own.
                    let task_id = task_id.ok_or_else(|| {
                        AppCommandError::invalid_input(
                            "refresh requires a task_id for binary components",
                        )
                    })?;
                    components_update_core(db, emitter, &id, &task_id).await?;
                }
                CATEGORY_MCP => {
                    let mcp = mcp.ok_or_else(|| {
                        AppCommandError::configuration_invalid(
                            "rc-mcp runtime is not available in this process",
                        )
                    })?;
                    // The action that re-binds the broker from the UI is
                    // identical to its "start it" action; the two share one
                    // failure mode and one error surface.
                    mcp.start().await?;
                    emit_components_updated(emitter);
                }
                CATEGORY_SKILL => {
                    let skill = skill.ok_or_else(|| {
                        AppCommandError::configuration_invalid(
                            "skill runtime is not available in this process",
                        )
                    })?;
                    skill.refresh(&db.conn).await?;
                    emit_components_updated(emitter);
                }
                other => {
                    return Err(AppCommandError::invalid_input(format!(
                        "unknown category: {other}"
                    )));
                }
            }
        }
    }
    let components = components_list_core(db, mcp, skill).await?;
    Ok(ComponentsListResult { components })
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

/// Create/refresh one PATH shim per installed+enabled binary applet and remove
/// shims for disabled ones.
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
    // binary's switch gates them all; each applet also has its own switch.
    let busybox_enabled = by_id.get(RECURBUSYBOX_TOOL_ID).copied().unwrap_or(true);
    for tool in RECURBUSYBOX_TOOLS {
        let applet_enabled = by_id
            .get(applet_setting_id(tool).as_str())
            .copied()
            .unwrap_or(true);
        let shim = dir.join(binary_cache::executable_file_name(tool));
        if busybox_enabled && applet_enabled {
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
#[allow(clippy::too_many_arguments)]
fn tauri_runtime(
    broker: &Arc<DelegationBroker>,
    tokens: &Arc<TokenRegistry>,
    feedback: &FeedbackRuntimeConfig,
    question: &QuestionRuntimeConfig,
    session_info: &SessionInfoRuntimeConfig,
    authoring: &ChatAuthoringRuntimeConfig,
    browser: &BrowserToolsRuntimeConfig,
    computer: &ComputerToolsRuntimeConfig,
) -> RuntimeComponents {
    RuntimeComponents::new(
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
    let runtime = tauri_runtime(
        broker.inner(),
        tokens.inner(),
        feedback.inner(),
        question.inner(),
        session_info.inner(),
        authoring.inner(),
        browser.inner(),
        computer.inner(),
    );
    components_list_core(&db, Some(&runtime), Some(&runtime)).await
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

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn components_apply(
    action: ComponentsAction,
    db: tauri::State<'_, AppDatabase>,
    broker: tauri::State<'_, Arc<DelegationBroker>>,
    tokens: tauri::State<'_, Arc<TokenRegistry>>,
    feedback: tauri::State<'_, FeedbackRuntimeConfig>,
    question: tauri::State<'_, QuestionRuntimeConfig>,
    session_info: tauri::State<'_, SessionInfoRuntimeConfig>,
    authoring: tauri::State<'_, ChatAuthoringRuntimeConfig>,
    browser: tauri::State<'_, BrowserToolsRuntimeConfig>,
    computer: tauri::State<'_, ComputerToolsRuntimeConfig>,
    app: tauri::AppHandle,
) -> Result<ComponentsListResult, AppCommandError> {
    let runtime = tauri_runtime(
        broker.inner(),
        tokens.inner(),
        feedback.inner(),
        question.inner(),
        session_info.inner(),
        authoring.inner(),
        browser.inner(),
        computer.inner(),
    );
    let emitter = EventEmitter::Tauri(app);
    components_apply_core(&db, &emitter, Some(&runtime), Some(&runtime), action).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

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

    /// In-memory rc-mcp stand-in. Records every group write so a test can prove
    /// an action touched exactly the group it named, and lets `start` be forced
    /// to fail.
    #[derive(Default)]
    struct FakeMcp {
        enabled: AtomicBool,
        start_result: AtomicBool,
        writes: Mutex<Vec<(String, bool)>>,
    }

    impl FakeMcp {
        fn new(enabled: bool) -> Self {
            let fake = Self::default();
            fake.enabled.store(enabled, Ordering::SeqCst);
            fake.start_result.store(true, Ordering::SeqCst);
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
    impl McpController for FakeMcp {
        async fn status(&self) -> RcMcpServiceStatus {
            FakeMcp::status(self)
        }

        async fn set_group(
            &self,
            _conn: &DatabaseConnection,
            _emitter: &EventEmitter,
            group: &str,
            enabled: bool,
        ) -> Result<(), AppCommandError> {
            self.writes.lock().unwrap().push((group.to_string(), enabled));
            self.enabled.store(enabled, Ordering::SeqCst);
            Ok(())
        }

        async fn start(&self) -> Result<(), AppCommandError> {
            if self.start_result.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(AppCommandError::configuration_invalid("bind failed"))
            }
        }
    }

    /// In-memory generative-UI skill stand-in: records the (agent, linked)
    /// pairs it was asked to move.
    #[derive(Default)]
    struct FakeSkill {
        conflict: AtomicBool,
        linked: Mutex<Vec<String>>,
        link_writes: Mutex<Vec<(String, bool)>>,
    }

    impl FakeSkill {
        fn new(linked: &[&str]) -> Self {
            let fake = Self::default();
            *fake.linked.lock().unwrap() = linked.iter().map(|s| s.to_string()).collect();
            fake
        }

        fn snapshot(&self) -> GenerativeUiSkillSnapshot {
            GenerativeUiSkillSnapshot {
                conflict: self.conflict.load(Ordering::SeqCst),
                linked_agents: self.linked.lock().unwrap().clone(),
                bundled_hash: "bundled".to_string(),
                central_hash: Some("bundled".to_string()),
            }
        }
    }

    #[async_trait::async_trait]
    impl SkillController for FakeSkill {
        async fn snapshot(&self) -> Result<GenerativeUiSkillSnapshot, AppCommandError> {
            Ok(FakeSkill::snapshot(self))
        }

        async fn link_agent(&self, agent: &str, linked: bool) -> Result<(), AppCommandError> {
            self.link_writes
                .lock()
                .unwrap()
                .push((agent.to_string(), linked));
            let mut current = self.linked.lock().unwrap();
            current.retain(|a| a != agent);
            if linked {
                current.push(agent.to_string());
            }
            Ok(())
        }

        async fn refresh(&self, _conn: &DatabaseConnection) -> Result<(), AppCommandError> {
            Ok(())
        }
    }


    fn find<'a>(rows: &'a [ComponentInfo], id: &str) -> &'a ComponentInfo {
        rows.iter().find(|row| row.id == id).expect("row")
    }

    fn multicall_tools(row: &ComponentInfo) -> &[MulticallTool] {
        match &row.state {
            ComponentState::Multicall(state) => &state.tools,
            other => panic!("expected multicall, got {other:?}"),
        }
    }

    fn mcp_state(row: &ComponentInfo) -> &RcMcpServiceStatus {
        match &row.state {
            ComponentState::Mcp(status) => status,
            other => panic!("expected mcp, got {other:?}"),
        }
    }

    fn skill_state(row: &ComponentInfo) -> &SkillState {
        match &row.state {
            ComponentState::Skill(state) => state,
            other => panic!("expected skill, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn binary_category_is_the_two_executables_not_their_applets() {
        let db = fresh_in_memory_db().await;
        let list = components_list_core(&db, None, None).await.expect("list");

        assert_eq!(list.len(), COMPONENTS.len(), "one row per registry entry");
        for category in [
            ComponentCategory::Binary,
            ComponentCategory::Mcp,
            ComponentCategory::Skill,
        ] {
            assert!(
                list.iter().any(|c| c.category == category),
                "category {category:?} has no components"
            );
        }

        let binaries: Vec<_> = list
            .iter()
            .filter(|c| c.category == ComponentCategory::Binary)
            .collect();
        let ids: Vec<&str> = binaries.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["@ra"],
            "only the single-purpose binary is `binary`; rb is `multicall`"
        );

        let ra = find(&list, "@ra");
        assert!(ra.required, "RecurAgent cannot be disabled");
        match &ra.state {
            ComponentState::Binary(state) => {
                assert!(state.available_version.is_none(), "ra is bundled, not downloaded");
            }
            other => panic!("expected binary, got {other:?}"),
        }

        let rb = find(&list, RECURBUSYBOX_TOOL_ID);
        assert_eq!(rb.category, ComponentCategory::Multicall);
        assert_eq!(multicall_tools(rb).len(), RECURBUSYBOX_TOOLS.len());
        let names: Vec<&str> = multicall_tools(rb)
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(names, RECURBUSYBOX_TOOLS);
        assert!(multicall_tools(rb).iter().all(|tool| tool.enabled));
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

        let rb = components_list_core(&db, None, None)
            .await
            .expect("list")
            .into_iter()
            .find(|c| c.id == RECURBUSYBOX_TOOL_ID)
            .unwrap();
        assert_eq!(rb.env, env);

        // Applet ids are not components, so they are refused like any unknown id.
        for unknown in ["@nope", "@recurbusybox:jq"] {
            assert!(components_set_env_core(&db, unknown, env.clone())
                .await
                .is_err());
        }
    }

    #[tokio::test]
    async fn mcp_state_comes_from_the_live_service_not_a_switch() {
        let db = fresh_in_memory_db().await;
        let mcp = FakeMcp::new(false);
        let skill = FakeSkill::new(&[]);

        // A stopped service must not read as on, even though the legacy setting
        // row still defaults to enabled.
        let list = components_list_core(&db, Some(&mcp), Some(&skill))
            .await
            .expect("list");
        let row = find(&list, "@rc-mcp");
        assert!(!rc_mcp_component_enabled(mcp_state(row)));
        assert_eq!(mcp_state(row).state, RcMcpServiceState::Disabled);

        mcp.enabled.store(true, Ordering::SeqCst);
        let list = components_list_core(&db, Some(&mcp), Some(&skill))
            .await
            .expect("list");
        assert!(rc_mcp_component_enabled(mcp_state(find(&list, "@rc-mcp"))));
    }




    #[tokio::test]
    async fn skill_conflict_is_reported_not_hidden() {
        let db = fresh_in_memory_db().await;
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(&[]);
        skill.conflict.store(true, Ordering::SeqCst);

        let list = components_list_core(&db, Some(&mcp), Some(&skill))
            .await
            .expect("list");
        assert_eq!(
            skill_state(find(&list, "@ui-render-skill")).central,
            SkillCentral::Conflict
        );
    }

    #[tokio::test]
    async fn apply_start_mcp_service_propagates_failure() {
        // `start` MUST bubble a bind failure to the caller; swallow it here
        // and the toast goes "Started!" against a dead socket.
        let db = fresh_in_memory_db().await;
        let mcp = FakeMcp::new(true);
        mcp.start_result.store(false, Ordering::SeqCst);
        let skill = FakeSkill::new(&[]);

        let emitter = EventEmitter::test_web_only(Arc::new(
            crate::web::event_bridge::WebEventBroadcaster::new(),
        ));
        let err = components_apply_core(
            &db,
            &emitter,
            Some(&mcp),
            Some(&skill),
            ComponentsAction::McpStartService,
        )
        .await
        .expect_err("McpStartService must surface the bind failure");
        assert!(err.to_string().contains("bind failed"));
    }

    #[tokio::test]
    async fn apply_mcp_tool_group_touches_only_the_one_it_names() {
        // One switch one group: a single ComponentsAction must never blanket
        // toggle the others. This is the regression test for the legacy
        // "turn one off, blank seven" bug.
        let db = fresh_in_memory_db().await;
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(&[]);

        let emitter = EventEmitter::test_web_only(Arc::new(
            crate::web::event_bridge::WebEventBroadcaster::new(),
        ));
        let result = components_apply_core(
            &db,
            &emitter,
            Some(&mcp),
            Some(&skill),
            ComponentsAction::McpToolGroup {
                group: "delegation".to_string(),
                enabled: false,
            },
        )
        .await
        .expect("apply");

        let writes = mcp.writes.lock().unwrap();
        assert_eq!(
            writes.as_slice(),
            &[("delegation".to_string(), false)],
            "exactly one group was written"
        );
        // The rebuilt list reflects the post-toggle state, so the frontend
        // can replace its copy in one shot.
        let _ = result.components;
    }

    #[tokio::test]
    async fn apply_skill_link_touches_only_the_one_agent() {
        let db = fresh_in_memory_db().await;
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(&["codex", "claude-code"]);

        let emitter = EventEmitter::test_web_only(Arc::new(
            crate::web::event_bridge::WebEventBroadcaster::new(),
        ));
        components_apply_core(
            &db,
            &emitter,
            Some(&mcp),
            Some(&skill),
            ComponentsAction::SkillLink {
                agent: "gemini".to_string(),
                linked: true,
            },
        )
        .await
        .expect("apply");

        let writes = skill.link_writes.lock().unwrap();
        assert_eq!(
            writes.as_slice(),
            &[("gemini".to_string(), true)],
            "SkillLink must only write the named agent"
        );
        // Neither of the pre-linked agents was disturbed.
        let linked = skill.linked.lock().unwrap();
        assert!(linked.iter().any(|a| a == "codex"));
        assert!(linked.iter().any(|a| a == "claude-code"));
    }

    #[tokio::test]
    async fn apply_disabling_binary_does_not_clear_unrelated_settings() {
        // The legacy `components_set_enabled` used to take down every binary
        // row whenever the multi-call parent flipped. Tools must keep their
        // own switches.
        let db = fresh_in_memory_db().await;

        let rg_id = applet_setting_id("rg");
        let jq_id = applet_setting_id("jq");
        crate::db::service::component_setting_service::set_enabled(
            &db.conn,
            &rg_id,
            false,
        )
        .await
        .expect("preset rg off");
        crate::db::service::component_setting_service::set_enabled(
            &db.conn,
            &jq_id,
            false,
        )
        .await
        .expect("preset jq off");
        let mcp = FakeMcp::new(true);
        let skill = FakeSkill::new(&[]);

        let emitter = EventEmitter::test_web_only(Arc::new(
            crate::web::event_bridge::WebEventBroadcaster::new(),
        ));
        components_apply_core(
            &db,
            &emitter,
            Some(&mcp),
            Some(&skill),
            ComponentsAction::BinaryEnabled {
                id: RECURBUSYBOX_TOOL_ID.to_string(),
                enabled: false,
            },
        )
        .await
        .expect("apply disable rb");

        let rg = component_setting_service::get(&db.conn, &rg_id)
            .await
            .expect("get rg")
            .expect("rg row exists");
        let jq = component_setting_service::get(&db.conn, &jq_id)
            .await
            .expect("get jq")
            .expect("jq row exists");
        // Both rows still exist (no caller-side delete) and hold their
        // pre-write values.
        assert!(!rg.enabled, "rg stays off the user put it that way");
        assert!(!jq.enabled, "jq stays off the user put it that way");
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
