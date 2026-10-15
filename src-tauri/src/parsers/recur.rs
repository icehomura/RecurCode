//! History parser for **RecurAgent** (the `ra` CLI).
//!
//! This file used to be a copy of [`crate::parsers::pi`] and read pi's
//! `type`/`id`/`parentId` event tree. RecurAgent is a different program with a
//! different session store, so that copy listed nothing but `~/.ra` never holds
//! pi-shaped records: reading it linearly yielded sessions with no header, no
//! turns, no titles. It is rewritten here against RecurAgent's real on-disk
//! format (verified against its source).
//!
//! # Where the sessions live
//!
//! RecurAgent's data root is `$RA_HOME`, else `~/.ra` (on Windows
//! `C:\Users\<you>\.ra`); RecurCode launches the agent with
//! `ra acp --data-dir ~/.ra`, which is the same directory. Session transcripts
//! are `.jsonl` files reachable from that root in any of these layouts, so the
//! parser walks the root RECURSIVELY and never hard-codes one shape:
//!
//! ```text
//! <data_dir>/sessions/<key>.jsonl                                  # legacy flat
//! <data_dir>/users/<base>/sessions/<topic>.jsonl                   # canonical per-user
//! <data_dir>/profiles/<id>/data/sessions/<key>.jsonl               # per-profile legacy
//! <data_dir>/profiles/<id>/data/users/<base>/sessions/<topic>.jsonl # per-profile canonical
//! ```
//!
//! Filenames percent-encode the session key: bytes outside `[A-Za-z0-9_-]`
//! become `%{BYTE:02X}` (`:` → `%3A`), and a key longer than 200 bytes is
//! truncated with a `_{FNV-1a-64:016X}` suffix. The encoding is one-way for long
//! keys, so the conversation id is read from the HEADER line's exact
//! `session_key` and the decoded filename is only a fallback.
//!
//! # The file format
//!
//! Line 1 is the header (`SessionMeta`, snake_case, untagged):
//!
//! ```json
//! {"schema_version":2,"session_key":"…","created_at":"…","updated_at":"…","sealed_segments":0,
//!  "topic":"…","title":"…","summary":"…","parent_key":"…","base_seq":0,
//!  "title_manual":false,"child_contracts":[]}
//! ```
//!
//! Every later line is either a **Message** or a **control record**, told apart
//! by their keys: a `kind` key is a control record, a `role` key is a Message,
//! otherwise a line carrying `schema_version`/`session_key` is the header. An
//! unknown or malformed line is skipped, never an error, so a partially-written
//! or forward-versioned file still lists.
//!
//! A Message is flat — `{"role","content","timestamp", optional
//! "tool_calls","tool_call_id","reasoning_content","media",
//! "client_message_id","thread_id"}` — and unlike pi there are NO content-part
//! arrays: `content` is a plain STRING for every role, thinking is the flat
//! `reasoning_content`, an assistant tool call is a `tool_calls[]` array, and a
//! tool result is its own row with `role:"tool"` plus the `tool_call_id` it
//! answers. Records are append-only, so line order IS conversation order; rows
//! are grouped into threads by `thread_id` (a user row's own thread id equals its
//! `client_message_id`), and a file with no `thread_id` at all is read in file
//! order.
//!
//! A control record `{"kind":"rollback","num_turns":N,"at":"…"}` retires the N
//! user turns that precede it: those turns are folded OUT of the conversation
//! before grouping, so a rewound-and-re-asked prompt does not render twice.
//!
//! The transcript carries no cwd, no model, and no token usage, so those fields
//! are `None` — the context meter falls back to the name-based guess (which has
//! nothing to guess from) rather than inventing a window.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;
use walkdir::WalkDir;

use crate::models::{
    AgentType, ContentBlock, ConversationDetail, ConversationSummary, ImageData, MessageRole,
    MessageTurn, TurnRole, UnifiedMessage,
};
use crate::parsers::{
    backfill_turn_durations, compute_session_stats, infer_context_window_max_tokens,
    latest_turn_total_usage_tokens, merge_context_window_stats, relocate_orphaned_tool_results,
    resolve_patch_line_numbers, structurize_read_tool_output, title_from_user_text, truncate_str,
    AgentParser, ParseError,
};

/// RecurAgent's data root — the directory walked for session `.jsonl` files.
///
/// `$RA_HOME` when set and non-empty, else `~/.ra`, mirroring
/// `ra_core::brand::state_home` and [`crate::commands::recur_config::recur_config_root`]
/// so the panel, the launched `ra acp --data-dir`, and this parser read one and
/// the same store. No tilde expansion is applied: the value is used as a path
/// verbatim (the shell that set it already expanded it), exactly as RecurAgent
/// itself does.
pub(crate) fn resolve_recur_sessions_dir() -> PathBuf {
    resolve_recur_sessions_dir_from(std::env::var_os("RA_HOME"), dirs::home_dir())
}

/// [`resolve_recur_sessions_dir`] as a pure function of its inputs, so tests
/// drive it without touching the process environment.
fn resolve_recur_sessions_dir_from(
    ra_home: Option<OsString>,
    home_dir: Option<PathBuf>,
) -> PathBuf {
    match ra_home
        .filter(|value| !value.is_empty())
        .and_then(|value| value.into_string().ok())
    {
        Some(dir) => PathBuf::from(dir),
        // `~/.ra`, and `.ra` relative when there is no home at all (a bare
        // container without $HOME) — still one directory, never the filesystem root.
        None => home_dir
            .map(|home| home.join(".ra"))
            .unwrap_or_else(|| PathBuf::from(".ra")),
    }
}

/// Length a session key may keep before the encoder truncates it and appends its
/// FNV-1a-64 hash (RecurAgent's own `MAX_KEY_LEN` + `_{hash:016X}`).
const ENCODED_KEY_LIMIT: usize = 200;
/// The hex-digest suffix length, without the leading `_`.
const KEY_HASH_SUFFIX_LEN: usize = 16;

/// The nibble value of an ASCII hex digit, or `None`.
fn hex_digit(byte: u8) -> Option<u8> {
    let lower = byte.to_ascii_lowercase();
    match lower {
        b'0'..=b'9' => Some(lower - b'0'),
        b'a'..=b'f' => Some(lower - b'a' + 10),
        _ => None,
    }
}

/// Decode a session filename stem back to (approximately) its session key:
/// drop the `_{16 hex}` hash suffix a truncated key acquired, then reverse the
/// `%{BYTE:02X}` escaping.
///
/// Lossy for a key that was truncated (the prefix plus a hash is all that
/// survives), which is why the header's `session_key` is preferred and this is
/// only the fallback id for a file with no readable header. An invalid `%`
/// escape is kept verbatim rather than dropped, so a stem that is not
/// percent-encoded at all (a topic like `default`, or a hand-placed fixture)
/// decodes to itself.
pub(crate) fn decode_recur_session_stem(stem: &str) -> String {
    let stem = strip_key_hash_suffix(stem);
    let bytes = stem.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // A well-formed escape is `%` plus TWO hex digits, so the lookahead bound
        // is `i + 2 < len` only in the sense that index i+2 must exist.
        let escape = if i + 2 < bytes.len() {
            match (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                (Some(hi), Some(lo)) if bytes[i] == b'%' => Some(hi << 4 | lo),
                _ => None,
            }
        } else {
            None
        };
        match escape {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    // The encoder escapes every non-unreserved byte, so a decoded key is ASCII;
    // lossy conversion keeps a hand-written non-ASCII stem intact instead of failing.
    String::from_utf8_lossy(&out).into_owned()
}

/// Remove a trailing `_{16 hex}` FNV digest from a truncated-key stem.
fn strip_key_hash_suffix(stem: &str) -> &str {
    // `_{hash}` is 17 bytes, and the encoder only ever truncates to 200 bytes or
    // fewer, so a stem at or under the limit cannot carry a real suffix — but the
    // suffix shape alone is a fair thing to strip for a shorter key too, and a
    // legitimate key ending in 16 hex digits after an underscore is rarer than
    // the truncation case it fixes.
    let bytes = stem.as_bytes();
    if bytes.len() <= ENCODED_KEY_LIMIT {
        return stem;
    }
    let split = bytes.len() - (KEY_HASH_SUFFIX_LEN + 1);
    if bytes.get(split) != Some(&b'_') {
        return stem;
    }
    let hash = &bytes[split + 1..];
    if hash.iter().all(u8::is_ascii_hexdigit) {
        &stem[..split]
    } else {
        stem
    }
}

/// `RecurAgent`'s sessions root, held by the parser. Named `base_dir` for
/// consistency with the other single-root parsers; it is the DATA root, not a
/// `sessions` folder, because the layouts nest below it.
pub struct RecurParser {
    base_dir: PathBuf,
}

impl RecurParser {
    pub fn new() -> Self {
        Self {
            base_dir: resolve_recur_sessions_dir(),
        }
    }

    /// Construct a parser pointed at an explicit data root (test fixtures and
    /// the `test-utils` feature). Everything below it is walked recursively, so
    /// a fixture may use any of the four real layouts.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn parse_summary(&self, path: &Path) -> Option<ConversationSummary> {
        let parsed = parse_session(path);
        // A file the parser does not recognise as a RecurAgent session — no
        // header — is not a conversation. This is also what keeps an unrelated
        // `.jsonl` under the data root (an audit or telemetry log) off the list.
        if !parsed.has_header {
            return None;
        }
        let started_at = parsed.first_ts?;

        Some(ConversationSummary {
            id: parsed.session_id.unwrap_or_else(|| fallback_id(path)),
            agent_type: AgentType::RecurAgent,
            // The transcript records no working directory, so there is no folder
            // to show — the sessions list groups these under the agent instead.
            folder_path: None,
            folder_name: None,
            title: resolve_title(parsed.title, parsed.first_user_text),
            started_at,
            ended_at: parsed.last_ts,
            message_count: parsed.message_count,
            // No model, no provider, no git branch in RecurAgent's transcript.
            model: None,
            git_branch: None,
            parent_id: parsed.parent_key,
            parent_tool_use_id: None,
            delegation_call_id: None,
        })
    }

    fn parse_detail(
        &self,
        path: &Path,
        conversation_id: &str,
    ) -> Result<ConversationDetail, ParseError> {
        let parsed = parse_session(path);

        let mut turns = group_into_turns(parsed.messages);
        relocate_orphaned_tool_results(&mut turns);
        structurize_read_tool_output(&mut turns);
        // No cwd in the transcript, so a patch's line numbers stay as written.
        resolve_patch_line_numbers(&mut turns, None);
        backfill_turn_durations(&mut turns, &[]);

        let used_tokens = latest_turn_total_usage_tokens(&turns);
        // RecurAgent persists neither a model nor a token count, so both sides of
        // the context meter are unknown and the meter is omitted rather than
        // guessed. Kept as a call (not a literal `None`) so the day the
        // transcript records a model it lights up with no surgery here.
        let max_tokens = infer_context_window_max_tokens(None);
        let session_stats =
            merge_context_window_stats(compute_session_stats(&turns), used_tokens, max_tokens);

        let summary = ConversationSummary {
            id: conversation_id.to_string(),
            agent_type: AgentType::RecurAgent,
            folder_path: None,
            folder_name: None,
            title: resolve_title(parsed.title.clone(), parsed.first_user_text.clone()),
            started_at: parsed.first_ts.unwrap_or_else(Utc::now),
            ended_at: parsed.last_ts,
            message_count: parsed.message_count,
            model: None,
            git_branch: None,
            parent_id: parsed.parent_key.clone(),
            parent_tool_use_id: None,
            delegation_call_id: None,
        };

        Ok(ConversationDetail {
            summary,
            turns,
            session_stats,
            transcript_watermark: None,
        })
    }
}

impl Default for RecurParser {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentParser for RecurParser {
    fn list_conversations(&self) -> Result<Vec<ConversationSummary>, ParseError> {
        let mut conversations = Vec::new();
        if !self.base_dir.exists() {
            return Ok(conversations);
        }

        for entry in WalkDir::new(&self.base_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !is_session_file(path) {
                continue;
            }
            if let Ok(Some(summary)) =
                super::summary_cache::get_or_parse(AgentType::RecurAgent, path, || {
                    Ok(self.parse_summary(path))
                })
            {
                conversations.push(summary);
            }
        }

        conversations.sort_by_key(|c| std::cmp::Reverse(c.started_at));
        Ok(conversations)
    }

    fn get_conversation(&self, conversation_id: &str) -> Result<ConversationDetail, ParseError> {
        if self.base_dir.exists() {
            for entry in WalkDir::new(&self.base_dir)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if !is_session_file(path) {
                    continue;
                }
                // Match the header `session_key` (the exact id); a file whose
                // header is unreadable can still be reached by its decoded stem.
                if session_id_of(path).as_deref() == Some(conversation_id) {
                    return self.parse_detail(path, conversation_id);
                }
            }
        }

        Err(ParseError::ConversationNotFound(
            conversation_id.to_string(),
        ))
    }
}

/// Whether a path is a candidate session file: a `.jsonl` that is a file.
/// Whether it IS a RecurAgent session is decided by its header (see
/// [`parse_session`]), not by its directory, because the four layouts differ.
fn is_session_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jsonl")
}

/// The accumulated result of scanning one session `.jsonl`.
#[derive(Default)]
struct SessionParse {
    /// Grouped, chronological messages ready for [`group_into_turns`].
    messages: Vec<UnifiedMessage>,
    first_ts: Option<DateTime<Utc>>,
    last_ts: Option<DateTime<Utc>>,
    /// Header `session_key` — the stable conversation id.
    session_id: Option<String>,
    /// Header `title`, else header `summary`.
    title: Option<String>,
    /// Header `parent_key` — the conversation this one was forked/delegated from.
    parent_key: Option<String>,
    /// First user prompt, truncated, for use as a fallback title.
    first_user_text: Option<String>,
    /// Number of Message rows kept after the rollback fold.
    message_count: u32,
    /// Whether line 1 parsed as a `SessionMeta` header.
    has_header: bool,
}

/// One Message row, kept with its file line index so the synthetic message ids
/// are stable across parses even when a rollback folds records away in between.
struct MessageRow {
    value: Value,
    line_idx: usize,
}

/// Parse a RecurAgent session `.jsonl` into a chronologically-ordered,
/// thread-grouped list of `UnifiedMessage`s plus session metadata.
///
/// Three passes over the lines: read and classify (header / Message / control),
/// fold the rollback control records into the Message list, then map the
/// survivors. Unknown or malformed lines are skipped so a forward-compatible or
/// partially-written log never panics and never blocks the rest of the file.
fn parse_session(path: &Path) -> SessionParse {
    let mut sp = SessionParse::default();
    let Ok(file) = fs::File::open(path) else {
        return sp;
    };

    let mut header: Option<Value> = None;
    let mut rows: Vec<MessageRow> = Vec::new();
    for (line_idx, line) in BufReader::new(file).lines().enumerate() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // Control record: `kind` is its discriminator. Only the rollback that
        // changes the conversation is handled; anything else (an unknown `kind`)
        // is skipped.
        if value.get("kind").is_some() {
            if value.get("kind").and_then(Value::as_str) == Some("rollback") {
                let num_turns =
                    value.get("num_turns").and_then(Value::as_u64).unwrap_or(0) as usize;
                fold_rollback(&mut rows, num_turns);
            }
            continue;
        }
        if value.get("role").and_then(Value::as_str).is_some() {
            rows.push(MessageRow { value, line_idx });
            continue;
        }
        // Header: `schema_version` / `session_key`, and only the FIRST such line
        // counts — a stray duplicate later in the file must not restart it.
        if header.is_none()
            && (value.get("schema_version").is_some() || value.get("session_key").is_some())
        {
            header = Some(value);
        }
    }

    let Some(header) = header else {
        // Not a RecurAgent session: an unrelated `.jsonl`, or a file whose header
        // never got written. Drop the rows rather than inventing a conversation.
        return sp;
    };
    sp.has_header = true;
    sp.session_id = string_field(&header, "session_key");
    sp.title = string_field(&header, "title").or_else(|| string_field(&header, "summary"));
    sp.parent_key = string_field(&header, "parent_key");
    // The header bounds the span before any message extends it.
    note_ts(&mut sp, field_ts(&header, "created_at"));
    note_ts(&mut sp, field_ts(&header, "updated_at"));

    sp.message_count = rows.len() as u32;
    for idx in order_rows_by_thread(&rows) {
        let row = &rows[idx];
        let ts = note_ts(&mut sp, field_ts(&row.value, "timestamp"));
        if let Some(message) = message_row_to_unified(row, ts, &mut sp) {
            sp.messages.push(message);
        }
    }

    sp
}

/// Apply a `{"kind":"rollback","num_turns":N}` control record: drop the last N
/// user turns accumulated so far, each turn being the user row and every record
/// written after it (its replies, tool calls, tool results) up to the next user
/// row.
///
/// Because the file is append-only and the rollback line is written AFTER the
/// turns it retires, "the rows so far" is exactly the set it applies to, and the
/// re-asked prompt that follows lands normally at the tail. Stopping early when
/// no user row is left keeps a malformed over-large `num_turns` from erasing
/// rows that carry no user turn at all.
fn fold_rollback(rows: &mut Vec<MessageRow>, num_turns: usize) {
    for _ in 0..num_turns {
        let Some(start) = rows.iter().rposition(|row| is_role(&row.value, "user")) else {
            break;
        };
        rows.truncate(start);
    }
}

/// The thread each message row belongs to, or `None` when it does not say.
///
/// A user row's `thread_id` equals its own `client_message_id`, so either key
/// roots a thread; the reply rows carry only `thread_id`.
fn row_thread_key(value: &Value) -> Option<String> {
    string_field(value, "thread_id").or_else(|| string_field(value, "client_message_id"))
}

/// Order message rows so each thread's records are contiguous, threads in order
/// of first appearance, records within a thread in file order.
///
/// File order is already the answer for the ordinary single-thread session (one
/// group, unchanged order) and for a legacy file that never wrote a `thread_id`
/// (no keys, so every row rides the current group); this only reorders a file
/// where threads interleave, which is what lets a tool result land with the
/// assistant call it answers instead of stranded after an unrelated turn.
fn order_rows_by_thread(rows: &[MessageRow]) -> Vec<usize> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    // The group rows without a thread key attach to: the last thread opened.
    let mut current: Option<usize> = None;

    for (idx, row) in rows.iter().enumerate() {
        let key = row_thread_key(&row.value);
        let slot = match &key {
            Some(key) => *by_key.entry(key.clone()).or_insert_with(|| {
                let slot = groups.len();
                groups.push(Vec::new());
                slot
            }),
            None => match current {
                Some(slot) => slot,
                None => {
                    let slot = groups.len();
                    groups.push(Vec::new());
                    slot
                }
            },
        };
        groups[slot].push(idx);
        // A row that does not name its thread is answering the turn most recently
        // opened — remembering its slot is what keeps a legacy file (no
        // `thread_id` anywhere) in one group, i.e. in file order.
        if key.is_none() || is_role(&row.value, "user") {
            current = Some(slot);
        }
    }

    let mut order = Vec::with_capacity(rows.len());
    for group in &groups {
        order.extend(group.iter().copied());
    }
    order
}

/// Map one RecurAgent Message row onto a [`UnifiedMessage`], or `None` when it
/// carries nothing renderable (an empty content string, an empty assistant
/// reply with no calls and no reasoning).
///
/// Roles map straight onto the unified model: `user` / `assistant` / `system`
/// keep their roles, and `tool` becomes `MessageRole::Tool` so its
/// [`ContentBlock::ToolResult`] threads into the assistant turn that called it
/// (see [`group_into_turns`]).
fn message_row_to_unified(
    row: &MessageRow,
    ts: DateTime<Utc>,
    sp: &mut SessionParse,
) -> Option<UnifiedMessage> {
    let value = &row.value;
    let idx = row.line_idx;
    let content = value
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if is_role(value, "user") {
        let images = media_to_images(value.get("media"));
        if content.trim().is_empty() && images.is_empty() {
            return None;
        }
        if sp.first_user_text.is_none() && !content.trim().is_empty() {
            sp.first_user_text = Some(title_from_user_text(content.trim()));
        }
        let mut blocks = Vec::with_capacity(1 + images.len());
        if !content.is_empty() {
            blocks.push(ContentBlock::Text {
                text: content.to_string(),
            });
        }
        blocks.extend(images.into_iter().map(|image| ContentBlock::Image {
            data: image.data,
            mime_type: image.mime_type,
            uri: image.uri,
        }));
        return Some(unified_message(
            format!("recur-user-{idx}"),
            MessageRole::User,
            blocks,
            ts,
        ));
    }

    if is_role(value, "assistant") {
        let mut blocks = Vec::new();
        // Reasoning first: every other parser puts thinking ahead of the reply it
        // produced, and the flat `reasoning_content` carries no order information.
        if let Some(reasoning) = string_field(value, "reasoning_content") {
            blocks.push(ContentBlock::Thinking { text: reasoning });
        }
        if !content.trim().is_empty() {
            blocks.push(ContentBlock::Text {
                text: content.to_string(),
            });
        }
        if let Some(calls) = value.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                // RecurAgent's per-call `metadata` blob is opaque, so it rides in
                // `ToolUse.meta` namespaced under one key, mirroring how the
                // delegation lifecycle stores its own object there.
                let meta = call
                    .get("metadata")
                    .filter(|metadata| metadata.is_object())
                    .cloned()
                    .map(|metadata| {
                        Value::Object([("recurAgent".to_string(), metadata)].into_iter().collect())
                    });
                blocks.push(ContentBlock::ToolUse {
                    tool_use_id: string_field(call, "id"),
                    tool_name: string_field(call, "name").unwrap_or_else(|| "unknown".to_string()),
                    input_preview: arguments_preview(call.get("arguments")),
                    status: None,
                    meta,
                });
            }
        }
        if blocks.is_empty() {
            return None;
        }
        return Some(unified_message(
            format!("recur-assistant-{idx}"),
            MessageRole::Assistant,
            blocks,
            ts,
        ));
    }

    if is_role(value, "tool") {
        // A tool result row. RecurAgent writes no error flag, so `is_error` is
        // false — the content string is the result text as the agent saw it.
        let output = (!content.is_empty()).then(|| truncate_str(content, 4000));
        return Some(unified_message(
            format!("recur-toolresult-{idx}"),
            MessageRole::Tool,
            vec![ContentBlock::ToolResult {
                tool_use_id: string_field(value, "tool_call_id"),
                output_preview: output,
                is_error: false,
                agent_stats: None,
                images: Vec::new(),
            }],
            ts,
        ));
    }

    if is_role(value, "system") {
        if content.trim().is_empty() {
            return None;
        }
        return Some(unified_message(
            format!("recur-system-{idx}"),
            MessageRole::System,
            vec![ContentBlock::Text {
                text: content.to_string(),
            }],
            ts,
        ));
    }

    // Any other role value: unknown to this format, skipped.
    None
}

/// Whether a record's `role` is `role`.
fn is_role(value: &Value, role: &str) -> bool {
    value.get("role").and_then(Value::as_str) == Some(role)
}

/// Serialize a `tool_calls[].arguments` value into a compact JSON preview,
/// truncated. `None` for a missing or null value; a pre-stringified argument
/// blob is passed through rather than double-encoded.
fn arguments_preview(arguments: Option<&Value>) -> Option<String> {
    let arguments = arguments?;
    let serialized = match arguments {
        Value::Null => return None,
        Value::String(text) => {
            if text.is_empty() {
                return None;
            }
            text.to_string()
        }
        other => serde_json::to_string(other).ok()?,
    };
    Some(truncate_str(&serialized, 4000))
}

/// Map a message's `media` array (image / URI strings) onto [`ImageData`],
/// best-effort: a `data:<mime>;base64,<…>` URL splits into mime + payload,
/// anything else is a URI with the mime guessed from its extension.
fn media_to_images(media: Option<&Value>) -> Vec<ImageData> {
    let Some(items) = media.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.as_str().map(str::trim).filter(|s| !s.is_empty()))
        .map(|entry| {
            if let Some(rest) = entry.strip_prefix("data:") {
                if let Some((mime, data)) = rest.split_once(";base64,") {
                    return ImageData {
                        data: data.to_string(),
                        mime_type: if mime.is_empty() {
                            "image/png".to_string()
                        } else {
                            mime.to_string()
                        },
                        uri: None,
                    };
                }
            }
            ImageData {
                data: String::new(),
                mime_type: mime_from_uri(entry),
                uri: Some(entry.to_string()),
            }
        })
        .collect()
}

/// Guess an image mime from a URI's extension; `image/png` otherwise, which is
/// what the renderer expects for a payload it cannot classify.
fn mime_from_uri(uri: &str) -> String {
    let tail = uri.rsplit(['/', '\\']).next().unwrap_or(uri);
    let mime = match tail.rsplit_once('.') {
        Some((_, ext)) => match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => "image/png",
        },
        None => "image/png",
    };
    mime.to_string()
}

/// A trimmed, non-empty string field, or `None`.
fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// An RFC-3339 `timestamp`-style field → `DateTime<Utc>`; a malformed or absent
/// value is `None`, never a panic.
fn field_ts(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<DateTime<Utc>>().ok())
}

/// Extend the session span with one timestamp and return a concrete timestamp
/// for the message (the record's own, else the latest seen, else now).
///
/// The span is a min/max, not "first seen / last seen": the header's
/// `updated_at` is the authoritative end and a message row can carry a slightly
/// older stamp, while a record whose `timestamp` failed to parse still inherits
/// the latest known time rather than being dropped.
fn note_ts(sp: &mut SessionParse, ts_raw: Option<DateTime<Utc>>) -> DateTime<Utc> {
    if let Some(ts) = ts_raw {
        sp.first_ts = Some(sp.first_ts.map_or(ts, |first| first.min(ts)));
        sp.last_ts = Some(sp.last_ts.map_or(ts, |last| last.max(ts)));
    }
    ts_raw.or(sp.last_ts).unwrap_or_else(Utc::now)
}

/// Read just a session file's conversation id without parsing the whole file:
/// the header's `session_key`, else the decoded filename stem.
fn session_id_of(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        // The header is line 1; only it can carry the id.
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            break;
        };
        if value.get("role").and_then(Value::as_str).is_some() || value.get("kind").is_some() {
            break;
        }
        if let Some(key) = string_field(&value, "session_key") {
            return Some(key);
        }
        break;
    }
    Some(fallback_id(path))
}

/// Recover a conversation id from a session filename: the stem, percent-decoded
/// and with any truncation hash suffix dropped.
fn fallback_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    decode_recur_session_stem(&stem)
}

/// Prefer the header's own title, falling back to the first user prompt.
fn resolve_title(title: Option<String>, first_user_text: Option<String>) -> Option<String> {
    title.or(first_user_text)
}

fn unified_message(
    id: String,
    role: MessageRole,
    content: Vec<ContentBlock>,
    ts: DateTime<Utc>,
) -> UnifiedMessage {
    UnifiedMessage {
        id,
        role,
        content,
        timestamp: ts,
        // RecurAgent's transcript records no usage and no model.
        usage: None,
        duration_ms: None,
        model: None,
        completed_at: Some(ts),
        agent_message_id: None,
    }
}

/// Group the ordered `UnifiedMessage`s into `MessageTurn`s: User and System
/// messages each become their own turn; an Assistant message starts a turn that
/// absorbs the immediately-following Tool messages (its tool results), stopping
/// at the next Assistant message to keep turns small for virtualization.
/// (Private copy mirroring the other single-file-per-session parsers.)
fn group_into_turns(messages: Vec<UnifiedMessage>) -> Vec<MessageTurn> {
    let mut turns = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let msg = &messages[i];

        if matches!(msg.role, MessageRole::User) {
            turns.push(MessageTurn {
                id: format!("turn-{}", turns.len()),
                role: TurnRole::User,
                blocks: msg.content.clone(),
                timestamp: msg.timestamp,
                usage: None,
                duration_ms: None,
                model: None,
                completed_at: msg.completed_at,
                agent_message_id: None,
            });
            i += 1;
        } else if matches!(msg.role, MessageRole::System) {
            turns.push(MessageTurn {
                id: format!("turn-{}", turns.len()),
                role: TurnRole::System,
                blocks: msg.content.clone(),
                timestamp: msg.timestamp,
                usage: None,
                duration_ms: None,
                model: None,
                completed_at: msg.completed_at,
                agent_message_id: None,
            });
            i += 1;
        } else {
            // Assistant or Tool — start a group and absorb following Tool messages.
            let mut blocks: Vec<ContentBlock> = msg.content.clone();
            let mut usage = msg.usage.clone();
            let mut duration_ms = msg.duration_ms;
            let mut turn_model = msg.model.clone();
            let timestamp = msg.timestamp;
            let mut completed_at = msg.completed_at;
            i += 1;

            while i < messages.len() && matches!(messages[i].role, MessageRole::Tool) {
                blocks.extend(messages[i].content.clone());
                if usage.is_none() {
                    usage = messages[i].usage.clone();
                }
                if duration_ms.is_none() {
                    duration_ms = messages[i].duration_ms;
                }
                if turn_model.is_none() {
                    turn_model = messages[i].model.clone();
                }
                if messages[i].completed_at.is_some() {
                    completed_at = messages[i].completed_at;
                }
                i += 1;
            }

            turns.push(MessageTurn {
                id: format!("turn-{}", turns.len()),
                role: TurnRole::Assistant,
                blocks,
                timestamp,
                usage,
                duration_ms,
                model: turn_model,
                completed_at,
                agent_message_id: None,
            });
        }
    }

    turns
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use tempfile::tempdir;

    /// Write one session JSONL at `<base>/<rel>`, creating parent dirs.
    fn write_session(base: &Path, rel: &str, records: &[Value]) -> PathBuf {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().expect("parent dir")).expect("create session dir");
        let mut file = fs::File::create(&path).expect("create jsonl");
        for record in records {
            writeln!(
                file,
                "{}",
                serde_json::to_string(record).expect("serialize")
            )
            .expect("write line");
        }
        path
    }

    /// A `SessionMeta` header line (line 1 of every real file).
    fn header(session_key: &str) -> Value {
        json!({
            "schema_version": 2,
            "session_key": session_key,
            "created_at": "2026-06-27T10:00:00.000Z",
            "updated_at": "2026-06-27T10:00:30.000Z",
            "sealed_segments": 0
        })
    }

    fn user(text: &str, ts: &str) -> Value {
        json!({"role":"user","content":text,"timestamp":ts})
    }

    fn assistant(text: &str, ts: &str) -> Value {
        json!({"role":"assistant","content":text,"timestamp":ts})
    }

    /// `get_conversation` on a single session written at the legacy flat layout.
    fn parse_one(records: &[Value], session_key: &str) -> ConversationDetail {
        let dir = tempdir().expect("tempdir");
        write_session(
            dir.path(),
            &format!("sessions/{session_key}.jsonl"),
            records,
        );
        RecurParser::with_base_dir(dir.path().to_path_buf())
            .get_conversation(session_key)
            .expect("detail")
    }

    /// Every `Text` block's text across a detail.
    fn text_blocks(detail: &ConversationDetail) -> Vec<String> {
        detail
            .turns
            .iter()
            .flat_map(|t| &t.blocks)
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn thinking_texts(detail: &ConversationDetail) -> Vec<String> {
        detail
            .turns
            .iter()
            .flat_map(|t| &t.blocks)
            .filter_map(|b| match b {
                ContentBlock::Thinking { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parses_a_plain_exchange_end_to_end() {
        let dir = tempdir().expect("tempdir");
        let session_key = "acp:conv-1";
        // The real encoder escapes `:` — the file is named with the encoded key.
        write_session(
            dir.path(),
            "sessions/acp%3Aconv-1.jsonl",
            &[
                header(session_key),
                user("fix the failing test", "2026-06-27T10:00:01.000Z"),
                assistant("Running it now.", "2026-06-27T10:00:02.000Z"),
                assistant(
                    "Done — the assertion was stale.",
                    "2026-06-27T10:00:09.000Z",
                ),
            ],
        );

        let parser = RecurParser::with_base_dir(dir.path().to_path_buf());

        let summaries = parser.list_conversations().expect("list");
        assert_eq!(summaries.len(), 1, "the session is listed");
        let summary = &summaries[0];
        assert_eq!(summary.agent_type, AgentType::RecurAgent);
        assert_eq!(
            summary.id, session_key,
            "id comes from the header, not the filename"
        );
        assert_eq!(
            summary.title.as_deref(),
            Some("fix the failing test"),
            "no header title → the first user prompt titles it"
        );
        assert_eq!(summary.message_count, 3, "user + two assistant rows");
        assert_eq!(
            summary.started_at,
            "2026-06-27T10:00:00.000Z".parse::<DateTime<Utc>>().unwrap(),
            "started_at is the header created_at"
        );
        assert_eq!(
            summary.ended_at,
            Some("2026-06-27T10:00:30.000Z".parse::<DateTime<Utc>>().unwrap()),
            "ended_at is the header updated_at"
        );
        assert_eq!(summary.model, None, "no model in the transcript");
        assert_eq!(summary.folder_path, None, "no cwd in the transcript");

        let detail = parser.get_conversation(session_key).expect("detail");
        assert_eq!(detail.summary.agent_type, AgentType::RecurAgent);
        let roles: Vec<String> = detail
            .turns
            .iter()
            .map(|t| format!("{:?}", t.role))
            .collect();
        assert_eq!(
            roles,
            vec!["User", "Assistant", "Assistant"],
            "line order is conversation order"
        );
        let texts = text_blocks(&detail);
        assert!(texts.iter().any(|t| t == "fix the failing test"));
        assert!(texts.iter().any(|t| t == "Done — the assertion was stale."));
        assert_eq!(detail.transcript_watermark, None);
        assert!(
            detail.turns.iter().all(|t| t.usage.is_none()),
            "RecurAgent persists no token usage"
        );
    }

    #[test]
    fn header_title_and_summary_take_priority_over_first_prompt() {
        let session_key = "titled";
        let with_title = parse_one(
            &[
                json!({"schema_version":2,"session_key":session_key,
                       "created_at":"2026-06-27T10:00:00.000Z","updated_at":"2026-06-27T10:00:01.000Z",
                       "title":"My title","summary":"My summary"}),
                user("ignored prompt", "2026-06-27T10:00:02.000Z"),
            ],
            session_key,
        );
        assert_eq!(with_title.summary.title.as_deref(), Some("My title"));

        let with_summary = parse_one(
            &[
                json!({"schema_version":2,"session_key":session_key,
                       "created_at":"2026-06-27T10:00:00.000Z","updated_at":"2026-06-27T10:00:01.000Z",
                       "summary":"My summary"}),
                user("ignored prompt", "2026-06-27T10:00:02.000Z"),
            ],
            session_key,
        );
        assert_eq!(with_summary.summary.title.as_deref(), Some("My summary"));
    }

    #[test]
    fn tool_calls_and_tool_results_thread_into_one_turn() {
        let session_key = "tools";
        let detail = parse_one(
            &[
                header(session_key),
                user("read the file", "2026-06-27T10:00:01.000Z"),
                json!({"role":"assistant","content":"Let me look.","timestamp":"2026-06-27T10:00:02.000Z",
                       "tool_calls":[
                         {"id":"call_1","name":"read","arguments":{"path":"src/main.rs"}},
                         {"id":"call_2","name":"bash","arguments":{"command":"cargo check"}}]}),
                json!({"role":"tool","content":"fn main() {}","timestamp":"2026-06-27T10:00:03.000Z",
                       "tool_call_id":"call_1"}),
                json!({"role":"tool","content":"Compiling","timestamp":"2026-06-27T10:00:04.000Z",
                       "tool_call_id":"call_2"}),
                assistant("Both checked out.", "2026-06-27T10:00:05.000Z"),
            ],
            session_key,
        );

        let uses: Vec<(Option<String>, String, Option<String>)> = detail
            .turns
            .iter()
            .flat_map(|t| &t.blocks)
            .filter_map(|b| match b {
                ContentBlock::ToolUse {
                    tool_use_id,
                    tool_name,
                    input_preview,
                    ..
                } => Some((
                    tool_use_id.clone(),
                    tool_name.clone(),
                    input_preview.clone(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(uses.len(), 2, "one ToolUse per tool_calls[] entry");
        assert_eq!(uses[0].0.as_deref(), Some("call_1"));
        assert_eq!(uses[0].1, "read");
        assert!(
            uses[0]
                .2
                .as_deref()
                .is_some_and(|p| p.contains("src/main.rs")),
            "arguments become a compact JSON preview, got: {:?}",
            uses[0].2
        );

        let results: Vec<(Option<String>, Option<String>, bool)> = detail
            .turns
            .iter()
            .flat_map(|t| &t.blocks)
            .filter_map(|b| match b {
                ContentBlock::ToolResult {
                    tool_use_id,
                    output_preview,
                    is_error,
                    ..
                } => Some((tool_use_id.clone(), output_preview.clone(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.as_deref(), Some("call_1"));
        assert_eq!(results[0].1.as_deref(), Some("fn main() {}"));
        assert!(!results[0].2, "a tool row is not marked an error");

        // Both calls and both results land in the single assistant turn.
        let assistant_turn = detail
            .turns
            .iter()
            .find(|t| {
                let uses = t
                    .blocks
                    .iter()
                    .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                    .count();
                let results = t
                    .blocks
                    .iter()
                    .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                    .count();
                uses == 2 && results == 2
            })
            .expect("one assistant turn holding both calls and both results");
        assert!(matches!(assistant_turn.role, TurnRole::Assistant));
        assert!(text_blocks(&detail).iter().any(|t| t == "Let me look."));
    }

    #[test]
    fn reasoning_content_becomes_a_thinking_block() {
        let session_key = "reasoning";
        let detail = parse_one(
            &[
                header(session_key),
                user("which option?", "2026-06-27T10:00:01.000Z"),
                json!({"role":"assistant","content":"option B","timestamp":"2026-06-27T10:00:02.000Z",
                       "reasoning_content":"weighing A against B"}),
            ],
            session_key,
        );
        assert_eq!(thinking_texts(&detail), vec!["weighing A against B"]);
        assert!(matches!(
            detail.turns[1].blocks[0],
            ContentBlock::Thinking { .. }
        ));
        assert!(matches!(
            detail.turns[1].blocks[1],
            ContentBlock::Text { .. }
        ));

        // An assistant row whose only payload is reasoning still renders — it is
        // content, not bookkeeping.
        let only_thinking = parse_one(
            &[
                header(session_key),
                json!({"role":"assistant","content":"","timestamp":"2026-06-27T10:00:02.000Z",
                       "reasoning_content":"thinking without saying"}),
            ],
            session_key,
        );
        assert_eq!(
            thinking_texts(&only_thinking),
            vec!["thinking without saying".to_string()]
        );
    }

    #[test]
    fn tool_call_row_with_empty_content_still_becomes_a_tool_use() {
        let session_key = "empty-content";
        let detail = parse_one(
            &[
                header(session_key),
                json!({"role":"assistant","content":"","timestamp":"2026-06-27T10:00:02.000Z",
                       "tool_calls":[{"id":"call_9","name":"write","arguments":{"path":"a.txt"}}]}),
                json!({"role":"tool","content":"ok","timestamp":"2026-06-27T10:00:03.000Z",
                       "tool_call_id":"call_9"}),
            ],
            session_key,
        );
        assert!(detail
            .turns
            .iter()
            .flat_map(|t| &t.blocks)
            .any(|b| matches!(
                b,
                ContentBlock::ToolUse { tool_name, tool_use_id, .. }
                    if tool_name == "write" && tool_use_id.as_deref() == Some("call_9")
            )));
        assert!(detail.turns.iter().flat_map(|t| &t.blocks).any(|b| matches!(
            b,
            ContentBlock::ToolResult { tool_use_id, output_preview, .. }
                if tool_use_id.as_deref() == Some("call_9") && output_preview.as_deref() == Some("ok")
        )));
    }

    /// A rollback retires the N user turns BEFORE it; the re-asked prompt that
    /// follows is the conversation.
    #[test]
    fn rollback_folds_out_the_retired_turns() {
        let session_key = "rollback";
        let detail = parse_one(
            &[
                header(session_key),
                user("first question", "2026-06-27T10:00:01.000Z"),
                assistant("first answer", "2026-06-27T10:00:02.000Z"),
                user("the typo'd question", "2026-06-27T10:00:03.000Z"),
                json!({"role":"assistant","content":"answering the typo","timestamp":"2026-06-27T10:00:04.000Z",
                       "tool_calls":[{"id":"c1","name":"bash","arguments":{"command":"typo"}}]}),
                json!({"role":"tool","content":"not found","timestamp":"2026-06-27T10:00:05.000Z",
                       "tool_call_id":"c1"}),
                json!({"kind":"rollback","num_turns":1,"at":"2026-06-27T10:00:06.000Z"}),
                user("the fixed question", "2026-06-27T10:00:07.000Z"),
                assistant("fixed answer", "2026-06-27T10:00:08.000Z"),
            ],
            session_key,
        );

        let texts = text_blocks(&detail);
        assert!(
            !texts.iter().any(|t| t.contains("typo")),
            "the rolled-back turn and its tool rows must not render, got: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "first question"),
            "earlier turns survive"
        );
        assert!(texts.iter().any(|t| t == "the fixed question"));
        assert!(texts.iter().any(|t| t == "fixed answer"));
        assert_eq!(
            detail.summary.message_count, 4,
            "count is the Message rows kept after the fold"
        );
        // Only the surviving bash call is gone with its turn.
        assert!(
            !detail.turns.iter().flat_map(|t| &t.blocks).any(
                |b| matches!(b, ContentBlock::ToolUse { tool_use_id: Some(id), .. } if id == "c1")
            ),
            "a rolled-back tool call goes with its turn"
        );
    }

    /// Rolling back more turns than exist removes what it can and leaves a file
    /// that still renders its header-bounded span rather than an error.
    #[test]
    fn rollback_over_fetching_does_not_lose_later_turns() {
        let session_key = "rollback-all";
        let detail = parse_one(
            &[
                header(session_key),
                user("gone", "2026-06-27T10:00:01.000Z"),
                assistant("gone too", "2026-06-27T10:00:02.000Z"),
                json!({"kind":"rollback","num_turns":5,"at":"2026-06-27T10:00:03.000Z"}),
                user("kept", "2026-06-27T10:00:04.000Z"),
            ],
            session_key,
        );
        let texts = text_blocks(&detail);
        assert_eq!(texts, vec!["kept".to_string()]);
    }

    /// Interleaved threads: each assistant/tool row groups with the user turn
    /// whose `thread_id` it carries, even when rows from two threads alternate.
    #[test]
    fn interleaved_threads_group_by_thread_id() {
        let session_key = "threads";
        let detail = parse_one(
            &[
                header(session_key),
                json!({"role":"user","content":"thread A question","timestamp":"2026-06-27T10:00:01.000Z",
                       "thread_id":"t-a","client_message_id":"t-a"}),
                json!({"role":"user","content":"thread B question","timestamp":"2026-06-27T10:00:02.000Z",
                       "thread_id":"t-b","client_message_id":"t-b"}),
                json!({"role":"assistant","content":"B answer","timestamp":"2026-06-27T10:00:03.000Z",
                       "thread_id":"t-b",
                       "tool_calls":[{"id":"b1","name":"bash","arguments":{"command":"b"}}]}),
                json!({"role":"assistant","content":"A answer","timestamp":"2026-06-27T10:00:04.000Z",
                       "thread_id":"t-a"}),
                json!({"role":"tool","content":"B output","timestamp":"2026-06-27T10:00:05.000Z",
                       "tool_call_id":"b1","thread_id":"t-b"}),
            ],
            session_key,
        );

        // Threads appear in order of their root question; each reply sits with its
        // own thread, so B's tool result never strands after A's turn.
        let flat: Vec<(String, Vec<String>)> = detail
            .turns
            .iter()
            .map(|t| {
                (
                    format!("{:?}", t.role),
                    t.blocks
                        .iter()
                        .map(|b| match b {
                            ContentBlock::Text { text } => text.clone(),
                            ContentBlock::ToolResult { output_preview, .. } => {
                                output_preview.clone().unwrap_or_default()
                            }
                            ContentBlock::ToolUse { tool_use_id, .. } => {
                                format!("use:{}", tool_use_id.clone().unwrap_or_default())
                            }
                            _ => String::new(),
                        })
                        .collect(),
                )
            })
            .collect();
        let roles: Vec<&str> = flat.iter().map(|(role, _)| role.as_str()).collect();
        assert_eq!(
            roles,
            vec!["User", "Assistant", "User", "Assistant"],
            "thread A first, then thread B, each with its reply"
        );
        let b_turn = &flat[3];
        assert!(
            b_turn.1.iter().any(|t| t == "B answer") && b_turn.1.iter().any(|t| t == "B output"),
            "B's answer and B's tool output share B's turn, got: {:?}",
            b_turn.1
        );
    }

    #[test]
    fn system_rows_become_their_own_system_turns() {
        let session_key = "system";
        let detail = parse_one(
            &[
                header(session_key),
                json!({"role":"system","content":"session started","timestamp":"2026-06-27T10:00:01.000Z"}),
                user("hi", "2026-06-27T10:00:02.000Z"),
            ],
            session_key,
        );
        let system: Vec<String> = detail
            .turns
            .iter()
            .filter(|t| matches!(t.role, TurnRole::System))
            .flat_map(|t| &t.blocks)
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(system, vec!["session started".to_string()]);
    }

    #[test]
    fn media_becomes_image_blocks() {
        let session_key = "media";
        let detail = parse_one(
            &[
                header(session_key),
                json!({"role":"user","content":"what is in this picture?","timestamp":"2026-06-27T10:00:01.000Z",
                       "media":["data:image/jpeg;base64,QUJD","/tmp/shot.png"]}),
                assistant("a cat", "2026-06-27T10:00:02.000Z"),
            ],
            session_key,
        );
        let images: Vec<(String, String, Option<String>)> = detail
            .turns
            .iter()
            .flat_map(|t| &t.blocks)
            .filter_map(|b| match b {
                ContentBlock::Image {
                    data,
                    mime_type,
                    uri,
                } => Some((data.clone(), mime_type.clone(), uri.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(images.len(), 2);
        assert_eq!(
            images[0],
            ("QUJD".to_string(), "image/jpeg".to_string(), None)
        );
        assert_eq!(
            images[1],
            (
                String::new(),
                "image/png".to_string(),
                Some("/tmp/shot.png".to_string())
            ),
            "a plain path rides as a URI with its extension-derived mime"
        );
    }

    /// All four on-disk layouts are reachable from the data root in one walk.
    #[test]
    fn discovers_every_session_layout() {
        let dir = tempdir().expect("tempdir");
        let base = dir.path();
        let layouts = [
            "sessions/legacy%2Fkey.jsonl",
            "users/my%3Abase/sessions/default.jsonl",
            "profiles/p1/data/sessions/legacy-profile.jsonl",
            "profiles/p1/data/users/my%3Abase/sessions/topic-x.jsonl",
        ];
        for (n, rel) in layouts.iter().enumerate() {
            write_session(
                base,
                rel,
                &[
                    json!({"schema_version":2,"session_key":format!("key-{n}"),
                           "created_at":"2026-06-27T10:00:00.000Z",
                           "updated_at":"2026-06-27T10:00:01.000Z"}),
                    user("prompt", "2026-06-27T10:00:02.000Z"),
                ],
            );
        }
        // An unrelated JSONL with no header must not be listed as a session.
        write_session(
            base,
            "audit.jsonl",
            &[json!({"event":"startup","at":"2026-06-27T10:00:00.000Z"})],
        );
        // Nor a non-jsonl file.
        fs::write(base.join("config.json"), "{}").expect("config");

        let summaries = RecurParser::with_base_dir(base.to_path_buf())
            .list_conversations()
            .expect("list");
        let mut ids: Vec<&str> = summaries.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["key-0", "key-1", "key-2", "key-3"],
            "every layout lists, the headerless log does not"
        );
    }

    #[test]
    fn percent_decode_reverses_the_filename_encoder() {
        assert_eq!(decode_recur_session_stem("acp%3Aconv-1"), "acp:conv-1");
        assert_eq!(decode_recur_session_stem("default"), "default");
        assert_eq!(decode_recur_session_stem("a%20b"), "a b");
        // An escaped `%` itself (`%25`) decodes to one `%`, not to an escape.
        assert_eq!(decode_recur_session_stem("100%25"), "100%");
        // A stray invalid escape is kept verbatim rather than dropped.
        assert_eq!(decode_recur_session_stem("100%zz"), "100%zz");
        // The encoder escapes every byte outside `[A-Za-z0-9_-]`, so a stem that
        // needs no escaping (a topic like `default`) decodes to itself.
        assert_eq!(decode_recur_session_stem("a-b_c"), "a-b_c");
    }

    /// A truncated key carries a `_{16 hex}` digest past the 200-byte limit; the
    /// stem fallback drops it. Shorter keys that merely LOOK like they carry one
    /// are left alone, because dropping a real suffix is worse than keeping a
    /// decorative one.
    #[test]
    fn hash_suffix_is_stripped_only_past_the_key_limit() {
        let long = format!("{}_{:016X}", "k".repeat(200), 0x1234_ABCD_u64);
        assert_eq!(long.len(), 217);
        assert_eq!(
            decode_recur_session_stem(&long),
            "k".repeat(200),
            "a truncated key decodes back to its stored prefix"
        );

        let short = "session_DEADBEEFDEADBEEF";
        assert_eq!(decode_recur_session_stem(short), short);
    }

    #[test]
    fn header_key_outranks_the_decoded_filename() {
        let dir = tempdir().expect("tempdir");
        // Filename claims one key, header another: the header wins, because the
        // encoder is lossy for a truncated key.
        write_session(
            dir.path(),
            "sessions/wrong%20name.jsonl",
            &[
                json!({"schema_version":2,"session_key":"the-real-key",
                       "created_at":"2026-06-27T10:00:00.000Z",
                       "updated_at":"2026-06-27T10:00:01.000Z"}),
                user("hi", "2026-06-27T10:00:02.000Z"),
            ],
        );
        let parser = RecurParser::with_base_dir(dir.path().to_path_buf());
        let summaries = parser.list_conversations().expect("list");
        assert_eq!(summaries[0].id, "the-real-key");
        assert!(parser.get_conversation("the-real-key").is_ok());
    }

    /// With no readable header id the decoded stem is still a usable fallback —
    /// including for a file whose header is missing, which is not listed at all.
    #[test]
    fn fallback_id_decodes_the_stem() {
        let path = Path::new("sessions/acp%3Aconv-9.jsonl");
        assert_eq!(fallback_id(path), "acp:conv-9");
    }

    #[test]
    fn malformed_and_unknown_lines_are_skipped() {
        let dir = tempdir().expect("tempdir");
        let session_key = "robust";
        let path = dir.path().join("sessions/robust.jsonl");
        fs::create_dir_all(path.parent().unwrap()).expect("dir");
        let mut file = fs::File::create(&path).expect("file");
        writeln!(file, "{}", header(session_key)).unwrap();
        writeln!(file, "{{ this is not valid json").unwrap();
        writeln!(file).unwrap();
        writeln!(
            file,
            "{}",
            json!({"role":"user","content":"hello","timestamp":"2026-06-27T10:00:01.000Z"})
        )
        .unwrap();
        // An unknown control record and an unknown role are ignored, not errors.
        writeln!(
            file,
            "{}",
            json!({"kind":"seal","at":"2026-06-27T10:00:02.000Z"})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"role":"mystery","content":"noise","timestamp":"2026-06-27T10:00:03.000Z"})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"role":"assistant","content":"hi","timestamp":"2026-06-27T10:00:04.000Z"})
        )
        .unwrap();

        let parser = RecurParser::with_base_dir(dir.path().to_path_buf());
        let summaries = parser.list_conversations().expect("list survives");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, session_key);
        let detail = parser
            .get_conversation(session_key)
            .expect("detail survives");
        assert_eq!(
            text_blocks(&detail),
            vec!["hello".to_string(), "hi".to_string()]
        );
        assert_eq!(
            detail.summary.message_count, 3,
            "the mystery role is a Message row"
        );
    }

    /// A bad timestamp must not lose the record: the span keeps what parsed and
    /// the message still renders with a concrete timestamp.
    #[test]
    fn unparseable_timestamps_do_not_drop_messages() {
        let session_key = "bad-ts";
        let detail = parse_one(
            &[
                header(session_key),
                json!({"role":"user","content":"hello","timestamp":"not-a-date"}),
                assistant("hi", "2026-06-27T10:00:05.000Z"),
            ],
            session_key,
        );
        assert_eq!(
            text_blocks(&detail),
            vec!["hello".to_string(), "hi".to_string()]
        );
        assert_eq!(
            detail.summary.started_at,
            "2026-06-27T10:00:00.000Z".parse::<DateTime<Utc>>().unwrap(),
            "the header still bounds the span"
        );
    }

    #[test]
    fn an_orphaned_tool_result_is_relocated_to_its_call() {
        let session_key = "orphan";
        let detail = parse_one(
            &[
                header(session_key),
                json!({"role":"tool","content":"late output","timestamp":"2026-06-27T10:00:01.000Z",
                       "tool_call_id":"call_x"}),
                json!({"role":"assistant","content":"","timestamp":"2026-06-27T10:00:02.000Z",
                       "tool_calls":[{"id":"call_x","name":"bash","arguments":{}}]}),
            ],
            session_key,
        );
        assert!(detail.turns.iter().any(|t| {
            let has_use = t.blocks.iter().any(|b| {
                matches!(
                    b,
                    ContentBlock::ToolUse { tool_use_id: Some(id), .. } if id == "call_x"
                )
            });
            let has_result = t.blocks.iter().any(|b| {
                matches!(
                    b,
                    ContentBlock::ToolResult { tool_use_id: Some(id), .. } if id == "call_x"
                )
            });
            has_use && has_result
        }));
    }

    #[test]
    fn unknown_conversation_is_not_found() {
        let dir = tempdir().expect("tempdir");
        let parser = RecurParser::with_base_dir(dir.path().to_path_buf());
        assert!(matches!(
            parser.get_conversation("nope"),
            Err(ParseError::ConversationNotFound(_))
        ));
    }

    #[test]
    fn nonexistent_base_dir_lists_nothing() {
        let parser = RecurParser::with_base_dir(PathBuf::from("/nonexistent/ra-data"));
        assert!(parser
            .list_conversations()
            .expect("list is infallible")
            .is_empty());
    }

    #[test]
    fn empty_session_file_is_not_listed() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("sessions")).expect("dir");
        fs::write(dir.path().join("sessions/empty.jsonl"), "").expect("write");
        let parser = RecurParser::with_base_dir(dir.path().to_path_buf());
        assert!(parser.list_conversations().expect("list").is_empty());
    }

    #[test]
    fn data_dir_prefers_ra_home_then_tilde_dot_ra() {
        assert_eq!(
            resolve_recur_sessions_dir_from(
                Some(OsString::from("/srv/ra-data")),
                Some(PathBuf::from("/home/demo"))
            ),
            PathBuf::from("/srv/ra-data")
        );
        assert_eq!(
            resolve_recur_sessions_dir_from(None, Some(PathBuf::from("/home/demo"))),
            PathBuf::from("/home/demo/.ra")
        );
        // An empty env value is not a configured root.
        assert_eq!(
            resolve_recur_sessions_dir_from(
                Some(OsString::new()),
                Some(PathBuf::from("/home/demo"))
            ),
            PathBuf::from("/home/demo/.ra")
        );
        // No home at all still yields a usable relative root, never the fs root.
        assert_eq!(
            resolve_recur_sessions_dir_from(None, None),
            PathBuf::from(".ra")
        );
    }

    #[test]
    fn parent_key_surfaces_as_the_conversation_parent() {
        let session_key = "child";
        let detail = parse_one(
            &[
                json!({"schema_version":2,"session_key":session_key,
                       "created_at":"2026-06-27T10:00:00.000Z",
                       "updated_at":"2026-06-27T10:00:01.000Z",
                       "parent_key":"parent-session"}),
                user("delegated task", "2026-06-27T10:00:02.000Z"),
            ],
            session_key,
        );
        assert_eq!(detail.summary.parent_id.as_deref(), Some("parent-session"));
    }

    #[test]
    fn long_arguments_are_truncated_to_the_preview_budget() {
        let session_key = "big-args";
        let blob = "x".repeat(5000);
        let detail = parse_one(
            &[
                header(session_key),
                json!({"role":"assistant","content":"","timestamp":"2026-06-27T10:00:02.000Z",
                       "tool_calls":[{"id":"c1","name":"write","arguments":{"content":blob}}]}),
            ],
            session_key,
        );
        let preview = detail
            .turns
            .iter()
            .flat_map(|t| &t.blocks)
            .find_map(|b| match b {
                ContentBlock::ToolUse { input_preview, .. } => input_preview.clone(),
                _ => None,
            })
            .expect("a tool use");
        // `truncate_str` keeps `max_len` chars and appends its `...` marker.
        assert!(
            preview.ends_with("..."),
            "not truncated: {}",
            &preview[..40.min(preview.len())]
        );
        assert!(
            preview.chars().count() <= 4003,
            "preview budget exceeded: {}",
            preview.chars().count()
        );
    }
}
