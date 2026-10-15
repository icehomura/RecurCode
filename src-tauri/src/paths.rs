//! Centralized resolution of rc-owned filesystem paths.
//!
//! Mirrors the conventions already used by `preferences.rs` (`~/.rc/`)
//! and `experts.rs` (`~/.rc/skills/`). New features that need a
//! user-scoped persistent directory should call into this module instead of
//! re-deriving `dirs::home_dir().join(".rc")` themselves.
//!
//! The `rc_*_root` data directories resolve `$RC_HOME`, then
//! `$RC_DATA_DIR`, then `~/.rc`. Both binaries export `RC_DATA_DIR`
//! before anything reads it — the desktop app from [`desktop_data_dir`] at the
//! top of `run()`, `rc-server` in `main` — so the data directory is what
//! they resolve to, and `~/.rc` only serves a process started without
//! either variable.

use std::path::{Path, PathBuf};

const RC_DIR_NAME: &str = ".rc";
const PETS_DIR_NAME: &str = "pets";
const BROWSER_PROFILES_DIR_NAME: &str = "browser-profiles";
const UPLOADS_DIR_NAME: &str = "uploads";
const LOGS_DIR_NAME: &str = "logs";
const TURN_TIMINGS_DIR_NAME: &str = "turn-timings";
const ACP_TRANSCRIPTS_DIR_NAME: &str = "acp-transcripts";
const BACKGROUNDS_DIR_NAME: &str = "backgrounds";

/// `$RC_HOME` if set (and non-empty), else `~/.rc/`.
///
/// Returns the relative `.rc` path when no home directory is available;
/// callers must still handle creation failures gracefully.
pub fn rc_home_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME))
}

/// Root directory for desktop-pet assets.
///
/// Resolution order:
/// 1. `$RC_HOME/pets` (explicit override, used in tests and custom installs)
/// 2. `$RC_DATA_DIR/pets` (the data directory, see the module docs)
/// 3. `~/.rc/pets` (neither set)
pub fn rc_pets_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom).join(PETS_DIR_NAME);
    }
    if let Some(data) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(data).join(PETS_DIR_NAME);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME).join(PETS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME).join(PETS_DIR_NAME))
}

/// Root directory for built-in browser profiles (WebView2 user-data folders
/// on Windows, WebKitGTK data directories on Linux; macOS keeps profiles in
/// WebKit's own store and never reads this).
///
/// Resolution order matches `rc_pets_root()`.
pub fn rc_browser_profiles_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom).join(BROWSER_PROFILES_DIR_NAME);
    }
    if let Some(data) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(data).join(BROWSER_PROFILES_DIR_NAME);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME).join(BROWSER_PROFILES_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME).join(BROWSER_PROFILES_DIR_NAME))
}

/// Root directory for attachments uploaded from the web client.
///
/// Resolution order matches `rc_pets_root()`:
/// 1. `$RC_HOME/uploads`
/// 2. `$RC_DATA_DIR/uploads` (the data directory, see the module docs)
/// 3. `~/.rc/uploads` (neither set)
///
/// Files in this directory are not garbage-collected by RecurCode itself —
/// later conversations may still reference them via `file://` URIs
/// embedded in session history. To bound the long-term footprint on
/// shared / multi-tenant servers, operators can set
/// `RC_UPLOAD_MAX_TOTAL_BYTES` (see `web::handlers::files`): new
/// uploads beyond the cap are rejected at the API boundary while
/// existing files stay readable.
///
/// **Concurrency contract:** the quota check uses a process-local
/// in-flight reservation counter to make `RC_UPLOAD_MAX_TOTAL_BYTES`
/// a hard ceiling within one `rc-server` process. Multiple
/// `rc-server` processes sharing the same uploads root (e.g.
/// horizontally-scaled containers mounted on the same volume) will
/// each enforce the cap independently and can collectively exceed it.
/// RecurCode is designed for single-process deployments; horizontal
/// scaling would require external coordination (file lock, Redis,
/// reverse-proxy quota) that this codebase does not provide.
pub fn rc_uploads_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom).join(UPLOADS_DIR_NAME);
    }
    if let Some(data) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(data).join(UPLOADS_DIR_NAME);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME).join(UPLOADS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME).join(UPLOADS_DIR_NAME))
}

/// Root directory for the user-selected workspace background image.
///
/// Resolution mirrors [`rc_pets_root`] exactly:
/// 1. `$RC_HOME/backgrounds` (explicit override)
/// 2. `$RC_DATA_DIR/backgrounds` (the data directory, see the module docs)
/// 3. `~/.rc/backgrounds` (neither set)
pub fn rc_backgrounds_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom).join(BACKGROUNDS_DIR_NAME);
    }
    if let Some(data) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(data).join(BACKGROUNDS_DIR_NAME);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME).join(BACKGROUNDS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME).join(BACKGROUNDS_DIR_NAME))
}

/// Root directory for application diagnostic logs (rotating files written by
/// the `tracing` file appender; see `crate::logging`).
///
/// Resolution mirrors [`rc_uploads_root`] exactly so logs land on the same
/// filesystem root as uploads/pets/the database:
/// 1. `$RC_HOME/logs` (explicit override)
/// 2. `$RC_DATA_DIR/logs` (the data directory, see the module docs)
/// 3. `~/.rc/logs` (neither set)
///
/// Pure env + `dirs::home_dir()`, so it is callable at the very start of a
/// process — before the database (or, in `rc-server`, the tokio runtime)
/// exists — which is exactly when the subscriber must be installed. Both
/// binaries export the data directory before that point, so the logs sit
/// beside the database however the process was started.
pub fn rc_logs_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom).join(LOGS_DIR_NAME);
    }
    if let Some(data) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(data).join(LOGS_DIR_NAME);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME).join(LOGS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME).join(LOGS_DIR_NAME))
}

/// Root directory for RecurCode's own per-turn timing observations (see
/// `crate::turn_timings`) — wall-clock turn spans the ACP connection layer
/// records for agents whose native session store carries no per-turn
/// timestamps (Cursor). Written by the live connection, read back by the
/// history parser.
///
/// Resolution mirrors [`rc_uploads_root`]:
/// 1. `$RC_HOME/turn-timings`
/// 2. `$RC_DATA_DIR/turn-timings` (the data directory, see the module docs)
/// 3. `~/.rc/turn-timings` (neither set)
pub fn rc_turn_timings_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom).join(TURN_TIMINGS_DIR_NAME);
    }
    if let Some(data) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(data).join(TURN_TIMINGS_DIR_NAME);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME).join(TURN_TIMINGS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME).join(TURN_TIMINGS_DIR_NAME))
}

/// Root directory for RecurCode's own ACP transcripts (see
/// `crate::acp_transcript`) — the raw `session/update` stream and outgoing
/// prompts recorded for **custom ACP agents**, which have no rc-side
/// transcript parser of their own. Written by the live connection, read back
/// by `crate::parsers::acp_native`.
///
/// Resolution mirrors [`rc_turn_timings_root`]:
/// 1. `$RC_HOME/acp-transcripts`
/// 2. `$RC_DATA_DIR/acp-transcripts` (the data directory, see the module docs)
/// 3. `~/.rc/acp-transcripts` (neither set)
pub fn rc_acp_transcripts_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(custom).join(ACP_TRANSCRIPTS_DIR_NAME);
    }
    if let Some(data) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(data).join(ACP_TRANSCRIPTS_DIR_NAME);
    }
    dirs::home_dir()
        .map(|h| h.join(RC_DIR_NAME).join(ACP_TRANSCRIPTS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(RC_DIR_NAME).join(ACP_TRANSCRIPTS_DIR_NAME))
}

/// The data directory the desktop app exports as `RC_DATA_DIR` at the top
/// of `run()`, before the logging init or anything else reads it: a preset
/// `RC_DATA_DIR` (the operator's, or inherited from the RecurCode that launched
/// this one), else the platform data directory joined with the bundle
/// identifier — exactly what Tauri's `app_data_dir()` returns, which is not
/// available until `setup`. Absolutized either way, like
/// [`resolve_effective_data_dir`]; `None` when there is no platform data
/// directory, where `setup` fails on `app_data_dir()` all the same.
///
/// Exporting it only in `setup`, as the desktop app used to, left the log
/// directory to depend on how the process was started: a launch from the
/// Start menu or the Dock wrote to `~/.rc/logs`, while an instance RecurCode
/// restarted (after an update) inherited the variable and wrote beside the
/// database — and the Settings page always opened the latter.
pub fn desktop_data_dir(
    preset: Option<&std::ffi::OsStr>,
    platform_data_dir: Option<&Path>,
    identifier: &str,
) -> Option<PathBuf> {
    if let Some(custom) = preset.filter(|s| !s.is_empty()) {
        return Some(crate::git_credential::absolutize(Path::new(custom)));
    }
    platform_data_dir.map(|dir| crate::git_credential::absolutize(&dir.join(identifier)))
}

/// Single source of truth for "where does the database live, and where
/// do `paths::*` resolve their roots against."
///
/// Resolution:
/// 1. If `RC_DATA_DIR` is set and non-empty, return its absolutized
///    form. Honors the operator's choice even on desktop, where a
///    pre-set env var should override Tauri's identifier-derived path.
/// 2. Otherwise return the absolutized form of `tauri_fallback` —
///    typically `app.path().app_data_dir()` on desktop or the
///    server's default data dir.
///
/// Always returns an absolute path (`absolutize` re-bases against the
/// process CWD if needed). Callers should treat the result as
/// authoritative and not re-read `RC_DATA_DIR` themselves; the
/// startup code in `lib.rs` / `bin/rc_server.rs` writes the
/// resolved value back to the env so subprocess inheritance works,
/// but the in-process source of truth is this function.
///
/// This exists because Tauri's `app.path().app_data_dir()` does **not**
/// consult `RC_DATA_DIR` — it returns the identifier-derived path
/// unconditionally. Call sites that pass `app_data_dir()` straight
/// into git credential helpers, ACP, terminal sessions, etc. would
/// otherwise generate scripts pointing at an empty DB when the
/// operator pre-set `RC_DATA_DIR` to a custom location.
pub fn resolve_effective_data_dir(tauri_fallback: &Path) -> PathBuf {
    if let Some(custom) = std::env::var_os("RC_DATA_DIR").filter(|s| !s.is_empty()) {
        return crate::git_credential::absolutize(Path::new(&custom));
    }
    crate::git_credential::absolutize(tauri_fallback)
}

/// Drop the Windows extended-length ("verbatim") prefix from a path, so the
/// plain form is what leaves this process.
///
/// `fs::canonicalize` on Windows resolves through `GetFinalPathNameByHandleW`
/// and **always** returns the verbatim form — `\\?\C:\…`, or `\\?\UNC\server\
/// share\…` for a network share. That is the right thing to keep feeding back
/// into Rust filesystem calls, but it must not escape into a value some other
/// layer parses: the frontend's `buildFileUri` normalizes `\` to `/`, which
/// turns `\\?\C:\…` into `//?/C:/…` and trips its UNC-authority branch,
/// yielding a uri that decodes back to the bogus path `?/C:/…` (issue #392).
/// Callers that hand a canonical path to a client should route it through
/// here first; the canonical value itself stays in use for jail checks.
///
/// Only the two prefixes with a plain equivalent are rewritten. `\\?\Volume{…}`
/// and other device paths have none, so they pass through untouched — as does
/// every path that never had a verbatim prefix, which on Unix is all of them
/// (the prefix is not valid syntax there, and an upload path cannot acquire
/// one: `sanitize_upload_filename` strips backslashes and the bucket name is
/// alphanumeric). Non-UTF-8 paths are returned as-is rather than risk a lossy
/// rewrite.
pub fn simplify_verbatim_path(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(rest) = strip_prefix_ignore_ascii_case(text, r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// `str::strip_prefix` with an ASCII-case-insensitive comparison. Windows
/// accepts `\\?\unc\` as readily as `\\?\UNC\`, and the casing a path picked
/// up is not ours to predict.
fn strip_prefix_ignore_ascii_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

// The resolver functions above depend on global env vars (`RC_HOME`,
// `RC_DATA_DIR`), so unit tests would need cross-test serialization to
// avoid races. The behaviour is covered end-to-end by `pets::*` tests which
// set `RC_HOME` inside a serialized test mutex; we deliberately don't
// duplicate that here. `simplify_verbatim_path` reads no env, so it is
// tested directly.
#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop app's data directory, decided before anything reads it: a
    /// preset `RC_DATA_DIR` wins, else the platform data directory joined
    /// with the bundle identifier — Tauri's `app_data_dir()`. Absolute either
    /// way, since it is exported to every subprocess.
    #[test]
    fn the_desktop_data_dir_is_the_preset_else_the_identifier_dir() {
        let platform = std::env::temp_dir().join("platform-data");
        let preset = std::env::temp_dir().join("preset-data");
        assert_eq!(
            desktop_data_dir(None, Some(&platform), "app.rc"),
            Some(platform.join("app.rc"))
        );
        assert_eq!(
            desktop_data_dir(Some(std::ffi::OsStr::new("")), Some(&platform), "app.rc"),
            Some(platform.join("app.rc")),
            "an empty preset is no preset"
        );
        assert_eq!(
            desktop_data_dir(Some(preset.as_os_str()), Some(&platform), "app.rc"),
            Some(preset.clone())
        );
        assert_eq!(
            desktop_data_dir(Some(preset.as_os_str()), None, "app.rc"),
            Some(preset)
        );
        assert_eq!(desktop_data_dir(None, None, "app.rc"), None);
        let relative = desktop_data_dir(Some(std::ffi::OsStr::new("data")), None, "app.rc")
            .expect("a relative preset");
        assert!(relative.is_absolute(), "{}", relative.display());
        assert!(relative.ends_with("data"));
    }

    /// The shape `fs::canonicalize` hands back on Windows, and the one that
    /// broke image attachments in issue #392.
    #[test]
    fn simplifies_a_verbatim_drive_path() {
        assert_eq!(
            simplify_verbatim_path(Path::new(r"\\?\C:\Users\song\.rc\uploads\b\img.png")),
            PathBuf::from(r"C:\Users\song\.rc\uploads\b\img.png")
        );
    }

    #[test]
    fn simplifies_a_verbatim_unc_path_in_either_casing() {
        assert_eq!(
            simplify_verbatim_path(Path::new(r"\\?\UNC\srv\share\img.png")),
            PathBuf::from(r"\\srv\share\img.png")
        );
        assert_eq!(
            simplify_verbatim_path(Path::new(r"\\?\unc\srv\share\img.png")),
            PathBuf::from(r"\\srv\share\img.png")
        );
    }

    /// Everything without a rewritable prefix must come back byte-identical —
    /// this is what keeps the Linux/macOS/Docker upload response unchanged.
    #[test]
    fn leaves_every_other_shape_untouched() {
        for path in [
            "/home/u/.rc/uploads/b/img.png",
            "/data/uploads/b/img.png",
            r"C:\Users\song\img.png",
            r"\\srv\share\img.png",
            r"\\?\Volume{7b2f1c40-0000-0000-0000-100000000000}\img.png",
            "relative/path.png",
        ] {
            assert_eq!(
                simplify_verbatim_path(Path::new(path)),
                PathBuf::from(path),
                "rewrote a path that has no verbatim prefix: {path}"
            );
        }
    }
}
