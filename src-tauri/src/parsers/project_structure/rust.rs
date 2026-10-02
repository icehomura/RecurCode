use std::collections::HashMap;

use super::ExtractedSymbol;

/// Extract Rust symbols: functions, structs, enums, traits, impl blocks (methods),
/// modules, and type aliases.
pub fn extract_symbols(content: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let mut current_impl_for: Option<String> = None;
    let mut brace_depth: i32 = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();

        // Track brace depth for impl blocks
        if current_impl_for.is_some() {
            brace_depth += trimmed.matches('{').count() as i32;
            let close = trimmed.matches('}').count() as i32;
            brace_depth -= close;
            if brace_depth <= 0 {
                current_impl_for = None;
                brace_depth = 0;
            }
        }

        // impl blocks → set context for methods inside
        if let Some(caps) = re_impl().captures(trimmed) {
            current_impl_for = Some(caps[1].to_string());
            brace_depth = 0;
            continue; // Don't extract the impl line itself as a symbol
        }

        // pub fn / fn
        if let Some(caps) = re_fn().captures(trimmed) {
            let name = caps[1].to_string();
            let vis = if trimmed.starts_with("pub") { "pub" } else { "private" };
            let mut meta = HashMap::new();
            meta.insert("visibility".into(), vis.into());
            if trimmed.contains("async") {
                meta.insert("async".into(), "true".into());
            }

            symbols.push(ExtractedSymbol {
                name,
                kind: if current_impl_for.is_some() {
                    "method".into()
                } else {
                    "function".into()
                },
                summary: Some(extract_fn_signature(trimmed)),
                line: Some(line_num),
                language: "rust".into(),
                parent_name: current_impl_for.clone(),
                metadata: Some(meta),
            });
            continue;
        }

        // pub struct / struct
        if let Some(caps) = re_struct().captures(trimmed) {
            let name = caps[1].to_string();
            let vis = if trimmed.starts_with("pub") { "pub" } else { "private" };
            let mut meta = HashMap::new();
            meta.insert("visibility".into(), vis.into());
            symbols.push(ExtractedSymbol {
                name,
                kind: "struct".into(),
                summary: None,
                line: Some(line_num),
                language: "rust".into(),
                parent_name: None,
                metadata: Some(meta),
            });
            continue;
        }

        // pub enum / enum
        if let Some(caps) = re_enum().captures(trimmed) {
            let name = caps[1].to_string();
            let vis = if trimmed.starts_with("pub") { "pub" } else { "private" };
            let mut meta = HashMap::new();
            meta.insert("visibility".into(), vis.into());
            symbols.push(ExtractedSymbol {
                name,
                kind: "enum".into(),
                summary: None,
                line: Some(line_num),
                language: "rust".into(),
                parent_name: None,
                metadata: Some(meta),
            });
            continue;
        }

        // pub trait / trait
        if let Some(caps) = re_trait().captures(trimmed) {
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name,
                kind: "trait".into(),
                summary: None,
                line: Some(line_num),
                language: "rust".into(),
                parent_name: None,
                metadata: None,
            });
            continue;
        }

        // pub mod / mod
        if let Some(caps) = re_mod().captures(trimmed) {
            if !trimmed.contains("use ") && !trimmed.contains("mod ") {
                // Skip use statements
            }
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name,
                kind: "module".into(),
                summary: None,
                line: Some(line_num),
                language: "rust".into(),
                parent_name: None,
                metadata: None,
            });
            continue;
        }

        // pub type / type alias
        if let Some(caps) = re_type_alias().captures(trimmed) {
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name,
                kind: "interface".into(),
                summary: Some(trimmed.to_string()),
                line: Some(line_num),
                language: "rust".into(),
                parent_name: None,
                metadata: None,
            });
        }
    }

    symbols
}

fn extract_fn_signature(line: &str) -> String {
    // Extract up to the opening brace or end of line
    if let Some(pos) = line.find('{') {
        line[..pos].trim().to_string()
    } else {
        line.trim().trim_end_matches(';').trim().to_string()
    }
}

// Regex patterns - compiled once via lazy_static/OnceLock
use std::sync::OnceLock;
use regex::Regex;

fn re_fn() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:pub\s+)?(?:\w+\s+)*fn\s+(\w+)").unwrap())
}

fn re_struct() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:pub\s+)?(?:\w+\s+)*struct\s+(\w+)").unwrap())
}

fn re_enum() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:pub\s+)?(?:\w+\s+)*enum\s+(\w+)").unwrap())
}

fn re_trait() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:pub\s+)?trait\s+(\w+)").unwrap())
}

fn re_mod() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:pub\s+)?mod\s+(\w+)").unwrap())
}

fn re_impl() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"impl(?:<[^>]+>)?\s+(?:\w+\s+for\s+)?(\w+)").unwrap())
}

fn re_type_alias() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:pub\s+)?type\s+(\w+)").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_basic_rust_symbols() {
        let code = r#"
pub fn hello_world() -> String {
    "hello".to_string()
}

pub struct Config {
    pub name: String,
}

enum Color {
    Red,
    Green,
}

trait Drawable {
    fn draw(&self);
}

impl Config {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}

mod utils {
    pub fn helper() {}
}
"#;
        let symbols = extract_symbols(code);
        assert!(symbols.iter().any(|s| s.name == "hello_world" && s.kind == "function"));
        assert!(symbols.iter().any(|s| s.name == "Config" && s.kind == "struct"));
        assert!(symbols.iter().any(|s| s.name == "Color" && s.kind == "enum"));
        assert!(symbols.iter().any(|s| s.name == "Drawable" && s.kind == "trait"));
        assert!(symbols.iter().any(|s| s.name == "new" && s.kind == "method"));
        assert!(symbols.iter().any(|s| s.name == "utils" && s.kind == "module"));
    }
}
