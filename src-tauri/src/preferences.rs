//! User-scoped preferences stored at `~/.rc/preferences.json`.
//!
//! These are settings that must be readable **before** the Tauri builder and
//! tokio runtime start (e.g. the webview rendering flags applied via
//! `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` on Windows and `WEBKIT_DISABLE_*`
//! on Linux). All access is synchronous I/O so the data must stay tiny.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const PREFERENCES_FILE_NAME: &str = "preferences.json";
const RC_DIR_NAME: &str = ".rc";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppPreferences {
    pub disable_hardware_acceleration: bool,
    /// Bundled RecurBusyBox tools (rg/jq/fd/…) take precedence over a
    /// same-named host binary. `true` keeps the long-standing behavior of
    /// prepending the components shim dir to the agent child's PATH;
    /// `false` turns the shims into a fallback that only resolves when the
    /// host has no copy of the tool. Toggled from the Components settings page;
    /// defaults to on so a fresh install inherits the current behavior.
    #[serde(default = "default_prefer_bundled_tools")]
    pub prefer_bundled_tools: bool,
}

/// Default for [`AppPreferences::prefer_bundled_tools`]. New users inherit the
/// long-standing "bundled tools win" behavior — turning the toggle off is an
/// explicit choice, never an accidental upgrade surprise. `pub(crate)` so the
/// settings-page test can pin both the serde default and the in-memory
/// default to the same value without re-declaring it.
pub(crate) fn default_prefer_bundled_tools() -> bool {
    true
}

/// Hand-written because `#[derive(Default)]` zero-initializes `bool`s, and the
/// new field's default is `true`. Both the serde-default (for rows stored
/// before this field existed) and `AppPreferences::default()` (used when the
/// preferences file is missing) need to agree on the same value — otherwise a
/// fresh install would read back `false` the moment any other field was saved.
impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            disable_hardware_acceleration: false,
            prefer_bundled_tools: default_prefer_bundled_tools(),
        }
    }
}

pub fn preferences_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(RC_DIR_NAME).join(PREFERENCES_FILE_NAME))
}

/// Read preferences synchronously. Missing / unreadable / malformed file
/// returns `Default::default()`. Errors are intentionally swallowed because
/// this is called on the startup hot-path; callers log if needed.
pub fn load() -> AppPreferences {
    let Some(path) = preferences_file_path() else {
        return AppPreferences::default();
    };
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => AppPreferences::default(),
        Err(err) => {
            tracing::error!("[Preferences] failed to read {}: {err}", path.display());
            AppPreferences::default()
        }
    }
}

pub fn save(prefs: &AppPreferences) -> io::Result<()> {
    let path = preferences_file_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory unavailable"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(prefs)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    fs::write(&path, serialized)
}
