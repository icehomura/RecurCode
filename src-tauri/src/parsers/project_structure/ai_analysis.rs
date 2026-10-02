//! Heuristic call-chain analysis for project structure scanning.
//!
//! Performs static analysis on `FileScanResult` data to detect import
//! relationships, type inheritance, and function call edges without
//! requiring an actual LLM call.

use std::collections::HashMap;

use serde::Serialize;

use super::{ExtractedSymbol, FileScanResult};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A directed edge representing a relationship between two files.
#[derive(Debug, Clone, Serialize)]
pub struct CallEdge {
    /// Source file path (the caller / importer).
    pub from_path: String,
    /// Target file path (the callee / imported module).
    pub to_path: String,
    /// Edge kind: "calls", "imports", "extends", "implements".
    pub kind: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Analyse a set of file scan results and return outgoing `CallEdge`s keyed
/// by source file path.
///
/// The analysis applies four heuristics in order:
/// 1. **Import detection** - parse `use`, `import`, `require` statements.
/// 2. **Type inheritance** - detect `extends`/`implements`/`impl`/`:` patterns.
/// 3. **Function calls** - naive string matching of function names inside
///    other function bodies.
/// 4. **Export analysis** - track exported symbols referenced elsewhere.
pub fn analyze_call_chains(file_results: &[FileScanResult]) -> HashMap<String, Vec<CallEdge>> {
    // Build lookup helpers shared across heuristics.
    let symbol_index = build_symbol_index(file_results);
    let path_index = build_path_index(file_results);

    let mut edges: HashMap<String, Vec<CallEdge>> = HashMap::new();

    for result in file_results {
        let file_edges = edges.entry(result.path.clone()).or_default();

        // 1. Import detection
        file_edges.extend(detect_imports(result, &path_index));

        // 2. Type inheritance
        file_edges.extend(detect_inheritance(result));

        // 3. Function call detection
        file_edges.extend(detect_function_calls(
            result,
            &symbol_index,
        ));

        // 4. Export cross-references
        file_edges.extend(detect_export_references(
            result,
            &symbol_index,
        ));
    }

    edges
}

/// Serialise a slice of `CallEdge`s to a JSON string suitable for storage in
/// the `call_relations` column.
pub fn serialize_call_edges(edges: &[CallEdge]) -> String {
    serde_json::to_string(edges).unwrap_or_else(|_| "[]".to_string())
}

// ---------------------------------------------------------------------------
// Internal helpers: index building
// ---------------------------------------------------------------------------

/// Map from symbol name -> (file path, kind) for every extracted symbol.
fn build_symbol_index(
    results: &[FileScanResult],
) -> HashMap<String, Vec<(String, String)>> {
    let mut index: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for result in results {
        for sym in &result.symbols {
            index
                .entry(sym.name.clone())
                .or_default()
                .push((result.path.clone(), sym.kind.clone()));
        }
    }
    index
}

/// Map from a "module name" (file stem / last path component) to its full
/// relative path.  Used to resolve import targets.
fn build_path_index(results: &[FileScanResult]) -> HashMap<String, String> {
    let mut index: HashMap<String, String> = HashMap::new();
    for result in results {
        if let Some(stem) = result.path.rsplit('/').next() {
            // Strip extension to get module name
            let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
            index
                .entry(stem.to_string())
                .or_insert_with(|| result.path.clone());
        }
        // Also index the full filename (with extension) for Go-style imports.
        if let Some(filename) = result.path.rsplit('/').next() {
            index
                .entry(filename.to_string())
                .or_insert_with(|| result.path.clone());
        }
    }
    index
}

// ---------------------------------------------------------------------------
// Heuristic 1: Import detection
// ---------------------------------------------------------------------------

/// Detect import / use / require statements and resolve them to file paths.
fn detect_imports(
    result: &FileScanResult,
    path_index: &HashMap<String, String>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    match result.language.as_str() {
        "rust" => edges.extend(extract_rust_imports(result, path_index)),
        "typescript" | "javascript" => {
            edges.extend(extract_ts_imports(result, path_index))
        }
        "python" => edges.extend(extract_python_imports(result, path_index)),
        "go" => edges.extend(extract_go_imports(result, path_index)),
        "java" | "kotlin" => edges.extend(extract_java_imports(result, path_index)),
        _ => {}
    }

    edges
}

fn extract_rust_imports(
    result: &FileScanResult,
    path_index: &HashMap<String, String>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    // Collect the full file content to scan import lines.  We re-read the
    // file only through the symbols' metadata summaries — but since
    // ExtractedSymbol doesn't store raw lines, we use a heuristic: the
    // module's top-level name can be resolved from `use crate::...` or
    // `use super::...` or `use <crate_name>::...`.
    //
    // Because we don't have the raw source here, we rely on metadata
    // provided by the extractor.  The Rust extractor stores the parent
    // module context implicitly through `parent_name` and we check for
    // crate/external references via symbol summaries.

    // For Rust, we can detect inter-module references by looking at
    // symbols whose summary mentions `use` paths or whose parent context
    // references another module.  This is conservative.
    for sym in &result.symbols {
        if let Some(ref summary) = sym.summary {
            // Look for "use <path>;" patterns in summaries
            for token in extract_use_tokens(summary) {
                if let Some(target_path) = resolve_module_path(&token, path_index) {
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: target_path,
                        kind: "imports".into(),
                    });
                }
            }
        }
    }

    edges
}

fn extract_ts_imports(
    result: &FileScanResult,
    path_index: &HashMap<String, String>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    for sym in &result.symbols {
        if let Some(ref summary) = sym.summary {
            for token in extract_import_tokens(summary) {
                if let Some(target_path) = resolve_module_path(&token, path_index) {
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: target_path,
                        kind: "imports".into(),
                    });
                }
            }
        }
    }

    // Also check metadata for "imported_from" entries (set by extractors
    // when they detect cross-module references).
    for sym in &result.symbols {
        if let Some(ref meta) = sym.metadata {
            if let Some(imported) = meta.get("imported_from") {
                if let Some(target_path) = resolve_module_path(imported, path_index) {
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: target_path,
                        kind: "imports".into(),
                    });
                }
            }
        }
    }

    edges
}

fn extract_python_imports(
    result: &FileScanResult,
    path_index: &HashMap<String, String>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    for sym in &result.symbols {
        if let Some(ref summary) = sym.summary {
            // Python "import X" or "from X import Y" patterns
            for token in extract_import_tokens(summary) {
                if let Some(target_path) = resolve_module_path(&token, path_index) {
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: target_path,
                        kind: "imports".into(),
                    });
                }
            }
        }
    }

    edges
}

fn extract_go_imports(
    result: &FileScanResult,
    path_index: &HashMap<String, String>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    for sym in &result.symbols {
        if let Some(ref summary) = sym.summary {
            for token in extract_import_tokens(summary) {
                if let Some(target_path) = resolve_module_path(&token, path_index) {
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: target_path,
                        kind: "imports".into(),
                    });
                }
            }
        }
    }

    edges
}

fn extract_java_imports(
    result: &FileScanResult,
    path_index: &HashMap<String, String>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    for sym in &result.symbols {
        if let Some(ref summary) = sym.summary {
            for token in extract_import_tokens(summary) {
                if let Some(target_path) = resolve_module_path(&token, path_index) {
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: target_path,
                        kind: "imports".into(),
                    });
                }
            }
        }
    }

    edges
}

// ---------------------------------------------------------------------------
// Heuristic 2: Type inheritance
// ---------------------------------------------------------------------------

/// Detect class / trait / interface inheritance and implementation.
fn detect_inheritance(
    result: &FileScanResult,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    for sym in &result.symbols {
        let meta = match sym.metadata {
            Some(ref m) => m,
            None => continue,
        };

        // "extends" metadata key (TS/Java classes)
        if let Some(parent) = meta.get("extends") {
            edges.push(CallEdge {
                from_path: result.path.clone(),
                to_path: parent.clone(),
                kind: "extends".into(),
            });
        }

        // "implements" metadata key (TS interfaces, Java interfaces)
        if let Some(ifaces) = meta.get("implements") {
            for iface in ifaces.split(',') {
                let iface = iface.trim();
                if !iface.is_empty() {
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: iface.to_string(),
                        kind: "implements".into(),
                    });
                }
            }
        }

        // Rust: "impl_for" metadata (impl Trait for Type)
        if let Some(trait_name) = meta.get("impl_for") {
            edges.push(CallEdge {
                from_path: result.path.clone(),
                to_path: trait_name.clone(),
                kind: "implements".into(),
            });
        }

        // Python: "bases" metadata (class Foo(Bar, Baz))
        if let Some(bases) = meta.get("bases") {
            for (i, base) in bases.split(',').enumerate() {
                let base = base.trim();
                if !base.is_empty() {
                    let kind = if i == 0 {
                        "extends".into()
                    } else {
                        "implements".into()
                    };
                    edges.push(CallEdge {
                        from_path: result.path.clone(),
                        to_path: base.to_string(),
                        kind,
                    });
                }
            }
        }
    }

    edges
}

// ---------------------------------------------------------------------------
// Heuristic 3: Function calls
// ---------------------------------------------------------------------------

/// For each function, check whether its body (approximated via summary +
/// metadata) mentions names of other known functions.
fn detect_function_calls(
    result: &FileScanResult,
    symbol_index: &HashMap<String, Vec<(String, String)>>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    // Collect all function/method names across the project for matching.
    let all_function_names: Vec<String> = symbol_index
        .iter()
        .filter(|(_, entries)| entries.iter().any(|(_, k)| k == "function" || k == "method"))
        .map(|(name, _)| name.clone())
        .collect();

    for sym in &result.symbols {
        // We only look inside functions / methods for call sites.
        if sym.kind != "function" && sym.kind != "method" {
            continue;
        }

        // Build the "body text" from summary + metadata values.
        let body_text = build_body_text(sym);

        for callee_name in &all_function_names {
            // Skip self-references.
            if callee_name == &sym.name {
                continue;
            }

            if body_text.contains(callee_name.as_str()) {
                // Resolve which file defines this callee.
                if let Some(entries) = symbol_index.get(callee_name.as_str()) {
                    for (target_path, _) in entries {
                        // Avoid self-file edges.
                        if target_path == &result.path {
                            continue;
                        }
                        edges.push(CallEdge {
                            from_path: result.path.clone(),
                            to_path: target_path.clone(),
                            kind: "calls".into(),
                        });
                    }
                }
            }
        }
    }

    edges
}

// ---------------------------------------------------------------------------
// Heuristic 4: Export cross-references
// ---------------------------------------------------------------------------

/// Detect when a symbol exported by one file is referenced (by name) in
/// another file's symbols.
fn detect_export_references(
    result: &FileScanResult,
    symbol_index: &HashMap<String, Vec<(String, String)>>,
) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    for sym in &result.symbols {
        let meta = match sym.metadata {
            Some(ref m) => m,
            None => continue,
        };

        // Only look at exported symbols.
        if meta.get("exported").map(|s| s.as_str()) != Some("true") {
            continue;
        }

        // Check if any other file references this exported symbol's name.
        if let Some(refs) = symbol_index.get(&sym.name) {
            for (target_path, _) in refs {
                if target_path == &result.path {
                    continue;
                }
                // Conservatively add the edge — the target file contains
                // a symbol with the same name, indicating a cross-file reference.
                edges.push(CallEdge {
                    from_path: result.path.clone(),
                    to_path: target_path.clone(),
                    kind: "calls".into(),
                });
            }
        }
    }

    edges
}

// ---------------------------------------------------------------------------
// Token extraction utilities
// ---------------------------------------------------------------------------

/// Extract module-path tokens from a `use`-style statement string.
/// E.g. `"use crate::models::foo;"` -> `["models::foo"]`
fn extract_use_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for word in text.split_whitespace() {
        // Strip trailing semicolons and braces
        let clean = word.trim_matches(|c| c == ';' || c == '{' || c == '}');
        if clean.contains("::") || clean.contains('.') {
            // Only include if it looks like a path, not a keyword.
            if !clean.starts_with("crate") && !clean.starts_with("super") && !clean.starts_with("self") {
                tokens.push(clean.to_string());
            }
        }
    }
    tokens
}

/// Extract module-path tokens from `import`/`require` style statements.
fn extract_import_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();

    // Look for quoted strings (import "foo", require("foo"))
    for segment in text.split(|c| c == '"' || c == '\'') {
        let clean = segment.trim();
        if clean.is_empty() || clean.starts_with("from ") || clean.starts_with("import ")
            || clean.starts_with("require")
        {
            continue;
        }
        if clean.contains('/') || clean.contains('.') || clean.contains("::") {
            tokens.push(clean.to_string());
        }
    }

    // Also look for bare paths after `import`/`from` keywords.
    let lower = text.to_lowercase();
    if lower.contains("import") || lower.contains("from") || lower.contains("require") {
        for word in text.split_whitespace() {
            let clean = word.trim_matches(|c| c == ';' || c == '(' || c == ')' || c == ',');
            if (clean.contains('/') || clean.contains('.')) && !clean.starts_with("//") {
                // Avoid duplicates
                if !tokens.iter().any(|t| t == clean) {
                    tokens.push(clean.to_string());
                }
            }
        }
    }

    tokens
}

/// Try to resolve a module token (e.g. `"models::foo"` or `"./utils"`)
/// to an actual file path using the path index.
fn resolve_module_path(
    token: &str,
    path_index: &HashMap<String, String>,
) -> Option<String> {
    // Try the last segment of a `::` or `/` separated path.
    if let Some(last_segment) = token.rsplit(|c| c == ':' || c == '/').next() {
        // Direct stem match
        if let Some(path) = path_index.get(last_segment) {
            return Some(path.clone());
        }
        // Try stripping common prefixes like "./", "../"
        let cleaned = last_segment.trim_start_matches(|c| c == '.' || c == '/');
        if let Some(path) = path_index.get(cleaned) {
            return Some(path.clone());
        }
    }

    // Try the whole token as a key.
    if let Some(path) = path_index.get(token) {
        return Some(path.clone());
    }

    let cleaned = token.trim_start_matches(|c| c == '.' || c == '/');
    path_index.get(cleaned).cloned()
}

/// Build a searchable body-text representation of a symbol from its summary
/// and metadata values.
fn build_body_text(sym: &ExtractedSymbol) -> String {
    let mut parts = Vec::new();
    if let Some(ref summary) = sym.summary {
        parts.push(summary.as_str());
    }
    if let Some(ref meta) = sym.metadata {
        for val in meta.values() {
            parts.push(val.as_str());
        }
    }
    parts.join(" ")
}
