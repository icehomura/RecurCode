use std::collections::HashMap;

use super::ExtractedSymbol;

/// Extract Python symbols: functions, classes, methods, decorators.
pub fn extract_symbols(content: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let mut current_class: Option<String> = None;
    let mut current_indent: usize = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();
        // Blank lines carry no indentation and must not end the current class
        // scope (PEP8 separates methods with them); only a real, dedented line
        // closes the class.
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        // Track class context by indentation
        if current_class.is_some() && indent <= current_indent {
            current_class = None;
        }

        // Class declaration
        if let Some(caps) = re_class().captures(trimmed) {
            let name = caps[1].to_string();
            let mut meta = HashMap::new();
            if trimmed.contains("dataclass") || line.contains("@dataclass") {
                meta.insert("dataclass".into(), "true".into());
            }
            symbols.push(ExtractedSymbol {
                name: name.clone(),
                kind: "class".into(),
                summary: None,
                line: Some(line_num),
                language: "python".into(),
                parent_name: None,
                metadata: Some(meta),
            });
            current_class = Some(name);
            current_indent = indent;
            continue;
        }

        // Function / method
        if let Some(caps) = re_fn().captures(trimmed) {
            let name = caps[1].to_string();
            let mut meta = HashMap::new();
            if trimmed.starts_with('@') || (line_idx > 0 && content.lines().nth(line_idx - 1).map_or(false, |l| l.trim().starts_with('@'))) {
                meta.insert("decorated".into(), "true".into());
            }

            symbols.push(ExtractedSymbol {
                name,
                kind: if current_class.is_some() {
                    "method".into()
                } else {
                    "function".into()
                },
                summary: Some(extract_fn_sig(trimmed)),
                line: Some(line_num),
                language: "python".into(),
                parent_name: current_class.clone(),
                metadata: Some(meta),
            });
            continue;
        }

        // Top-level assignment (constants, etc.)
        if let Some(caps) = re_assign().captures(trimmed) {
            let name = caps[1].to_string();
            if name.chars().all(|c| c.is_uppercase() || c == '_' || c.is_numeric()) {
                symbols.push(ExtractedSymbol {
                    name,
                    kind: "struct".into(),
                    summary: None,
                    line: Some(line_num),
                    language: "python".into(),
                    parent_name: None,
                    metadata: None,
                });
            }
        }
    }

    symbols
}

fn extract_fn_sig(line: &str) -> String {
    if let Some(pos) = line.find(':') {
        line[..pos].trim().to_string()
    } else {
        line.trim().to_string()
    }
}

use std::sync::OnceLock;
use regex::Regex;

fn re_class() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"class\s+(\w+)").unwrap())
}

fn re_fn() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:async\s+)?def\s+(\w+)").unwrap())
}

fn re_assign() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\w+)\s*[=:]").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_python_symbols() {
        let code = r#"
MAX_RETRIES = 3

class Config:
    def __init__(self, name: str):
        self.name = name

    def validate(self) -> bool:
        return True

def create_config() -> Config:
    return Config("default")

async def fetch_data():
    pass
"#;
        let symbols = extract_symbols(code);
        assert!(symbols.iter().any(|s| s.name == "Config" && s.kind == "class"));
        assert!(symbols.iter().any(|s| s.name == "create_config" && s.kind == "function"));
        assert!(symbols.iter().any(|s| s.name == "fetch_data" && s.kind == "function"));
        assert!(symbols.iter().any(|s| s.name == "__init__" && s.kind == "method"));
        assert!(symbols.iter().any(|s| s.name == "validate" && s.kind == "method"));
        assert!(symbols.iter().any(|s| s.name == "MAX_RETRIES" && s.kind == "struct"));
    }
}
