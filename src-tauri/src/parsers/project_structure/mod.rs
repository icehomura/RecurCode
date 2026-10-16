//! Project structure scanner: extracts symbols from source files and builds
//! a hierarchical tree for the mind-map visualization.
//!
//! Language-specific extractors live in sibling modules. This module provides
//! the shared types, language detection, filesystem walker, and orchestrator
//! that ties everything together.

pub mod rust;
pub mod typescript;
pub mod python;
pub mod go;
pub mod java_kotlin;
pub mod ai_analysis;

use std::collections::HashMap;
use std::path::Path;

use crate::db::service::project_structure_service;
use crate::db::AppDatabase;
use crate::models::project_structure::{ProjectStructureNodeDraft, ProjectStructureNodeInfo};

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

/// A single extracted symbol from a source file.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    /// Symbol name (function name, class name, etc.)
    pub name: String,
    /// Node kind: "function", "class", "method", "struct", "enum", "trait", "interface", "module"
    pub kind: String,
    /// First line or short description
    pub summary: Option<String>,
    /// Line number in the source file (1-based)
    pub line: Option<u32>,
    /// Language identifier
    pub language: String,
    /// For nested symbols: parent symbol name
    pub parent_name: Option<String>,
    /// Extra metadata (e.g., visibility, parameters)
    pub metadata: Option<HashMap<String, String>>,
}

/// Result of scanning a single file.
#[derive(Debug, Clone)]
pub struct FileScanResult {
    /// Relative path from project root
    pub path: String,
    /// Language detected
    pub language: String,
    /// Extracted symbols
    pub symbols: Vec<ExtractedSymbol>,
}

/// Language detection from file extension.
pub fn detect_language(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some("rust"),
        "ts" | "tsx" | "mts" | "mjs" => Some("typescript"),
        "js" | "jsx" | "cjs" => Some("javascript"),
        "py" | "pyi" => Some("python"),
        "go" => Some("go"),
        "java" => Some("java"),
        "kt" | "kts" => Some("kotlin"),
        "vue" => Some("vue"),
        "svelte" => Some("svelte"),
        "html" | "htm" => Some("html"),
        "css" | "scss" | "less" => Some("css"),
        "json" | "toml" | "yaml" | "yml" => Some("config"),
        "md" | "rst" | "txt" => Some("doc"),
        _ => None,
    }
}

/// Whether a file should be scanned (not ignored, not binary, not too large).
pub fn should_scan_file(path: &Path, max_size: u64) -> bool {
    // Skip common non-source directories
    let path_str = path.to_string_lossy();
    let skip_patterns = [
        "node_modules",
        ".git",
        "target",
        "dist",
        "build",
        ".next",
        "__pycache__",
        ".venv",
        "vendor",
        ".idea",
        ".vscode",
    ];
    for pattern in &skip_patterns {
        if path_str.contains(&format!("/{pattern}/")) || path_str.contains(&format!("\\{pattern}\\")) {
            return false;
        }
    }

    // Check file size
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > max_size {
            return false;
        }
    } else {
        return false;
    }

    true
}

// ---------------------------------------------------------------------------
// Scan orchestrator
// ---------------------------------------------------------------------------

/// Scan a project directory and return extracted symbols as DB-ready drafts.
pub async fn scan_project(
    root: &Path,
    folder_id: i32,
    db: &AppDatabase,
) -> Result<Vec<ProjectStructureNodeInfo>, String> {
    let max_file_size: u64 = 512 * 1024; // 512KB per file
    let mut file_results: Vec<FileScanResult> = Vec::new();

    // Walk the filesystem
    walk_project(root, root, &mut file_results, max_file_size)?;

    // Convert to DB drafts with parent-child relationships
    let drafts = build_draft_tree(&file_results, folder_id);

    // Upsert into database
    project_structure_service::upsert_bulk(&db.conn, folder_id, drafts)
        .await
        .map_err(|e| format!("DB error: {e}"))
}

/// Recursively walk a directory and scan source files.
fn walk_project(
    dir: &Path,
    root: &Path,
    results: &mut Vec<FileScanResult>,
    max_file_size: u64,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("Cannot read {dir:?}: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            if should_scan_file(&path, u64::MAX) {
                walk_project(&path, root, results, max_file_size)?;
            }
        } else if let Some(lang) = detect_language(&path) {
            if should_scan_file(&path, max_file_size) {
                if let Ok(result) = scan_file(&path, lang, root) {
                    results.push(result);
                }
            }
        }
    }

    Ok(())
}

/// Scan a single file using the appropriate language extractor.
fn scan_file(path: &Path, language: &str, root: &Path) -> Result<FileScanResult, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {path:?}: {e}"))?;

    let rel_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    let symbols = match language {
        "rust" => rust::extract_symbols(&content),
        "typescript" | "javascript" => typescript::extract_symbols(&content, language),
        "python" => python::extract_symbols(&content),
        "go" => go::extract_symbols(&content),
        "java" | "kotlin" => java_kotlin::extract_symbols(&content, language),
        _ => Vec::new(), // Config/doc files: no symbols extracted
    };

    Ok(FileScanResult {
        path: rel_path,
        language: language.to_string(),
        symbols,
    })
}

/// Convert flat file scan results into hierarchical DB drafts.
///
/// Creates folder nodes for each directory, file nodes for each file,
/// and symbol nodes nested under their parent file.
fn build_draft_tree(
    file_results: &[FileScanResult],
    _folder_id: i32,
) -> Vec<ProjectStructureNodeDraft> {
    let mut drafts: Vec<ProjectStructureNodeDraft> = Vec::new();
    let mut path_to_id: HashMap<String, usize> = HashMap::new();
    let mut sort_counter: i32 = 0;

    // Track which folders we've already created
    let mut created_folders: HashSet<String> = HashSet::new();

    for file_result in file_results {
        let parts: Vec<&str> = file_result.path.split('/').collect();

        // Create folder nodes for each directory level
        let mut current_path = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                break; // Last part is the filename, not a folder
            }
            if current_path.is_empty() {
                current_path = part.to_string();
            } else {
                current_path = format!("{current_path}/{part}");
            }

            if !created_folders.contains(&current_path) {
                let parent_id = if current_path.contains('/') {
                    let parent = current_path.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
                    path_to_id.get(&parent).copied().map(|id| id as i32)
                } else {
                    None
                };

                let id = drafts.len();
                path_to_id.insert(current_path.clone(), id);
                created_folders.insert(current_path.clone());

                drafts.push(ProjectStructureNodeDraft {
                    path: current_path.clone(),
                    kind: "folder".to_string(),
                    name: part.to_string(),
                    parent_id,
                    language: None,
                    summary: None,
                    call_relations: None,
                    metadata: None,
                    sort_order: Some(sort_counter),
                });
                sort_counter += 1;
            }
        }

        // Create file node
        let file_parent_id = if parts.len() > 1 {
            let dir_path = parts[..parts.len() - 1].join("/");
            path_to_id.get(&dir_path).copied().map(|id| id as i32)
        } else {
            None
        };

        let file_id = drafts.len();
        let file_name = parts.last().unwrap_or(&"").to_string();
        path_to_id.insert(file_result.path.clone(), file_id);

        drafts.push(ProjectStructureNodeDraft {
            path: file_result.path.clone(),
            kind: "file".to_string(),
            name: file_name,
            parent_id: file_parent_id,
            language: Some(file_result.language.clone()),
            summary: Some(format!("{} symbols", file_result.symbols.len())),
            call_relations: None,
            metadata: None,
            sort_order: Some(sort_counter),
        });
        sort_counter += 1;

        // Create symbol nodes under the file
        for symbol in &file_result.symbols {
            let symbol_id = drafts.len();
            let symbol_path = format!("{}::{}", file_result.path, symbol.name);

            // If symbol has a parent (e.g., method inside class), nest under parent
            let parent_id = if let Some(ref parent_name) = symbol.parent_name {
                // Find the parent symbol in the same file
                drafts.iter().position(|d| {
                    d.path.starts_with(&file_result.path)
                        && d.name == *parent_name
                        && d.kind != "file"
                        && d.kind != "folder"
                }).map(|id| id as i32)
                    .or(Some(file_id as i32))
            } else {
                Some(file_id as i32)
            };

            path_to_id.insert(symbol_path.clone(), symbol_id);

            let metadata_json = symbol.metadata.as_ref().map(|m| {
                serde_json::to_string(m).unwrap_or_default()
            });

            drafts.push(ProjectStructureNodeDraft {
                path: symbol_path,
                kind: symbol.kind.clone(),
                name: symbol.name.clone(),
                parent_id,
                language: Some(symbol.language.clone()),
                summary: symbol.summary.clone(),
                call_relations: None,
                metadata: metadata_json,
                sort_order: Some(sort_counter),
            });
            sort_counter += 1;
        }
    }

    drafts
}

use std::collections::HashSet;
