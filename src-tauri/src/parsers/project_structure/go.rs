use std::collections::HashMap;

use super::ExtractedSymbol;

/// Extract Go symbols: functions, methods, types, interfaces, structs.
pub fn extract_symbols(content: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let mut current_type: Option<String> = None;

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();

        // Package declaration
        if let Some(caps) = re_package().captures(trimmed) {
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name,
                kind: "module".into(),
                summary: None,
                line: Some(line_num),
                language: "go".into(),
                parent_name: None,
                metadata: None,
            });
            continue;
        }

        // Interface type
        if let Some(caps) = re_interface().captures(trimmed) {
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name: name.clone(),
                kind: "interface".into(),
                summary: None,
                line: Some(line_num),
                language: "go".into(),
                parent_name: None,
                metadata: None,
            });
            current_type = Some(name);
            continue;
        }

        // Struct type
        if let Some(caps) = re_struct().captures(trimmed) {
            let name = caps[1].to_string();
            let mut meta = HashMap::new();
            if trimmed.contains("json:") || trimmed.contains("json:") {
                meta.insert("json_tag".into(), "true".into());
            }
            symbols.push(ExtractedSymbol {
                name: name.clone(),
                kind: "struct".into(),
                summary: None,
                line: Some(line_num),
                language: "go".into(),
                parent_name: None,
                metadata: Some(meta),
            });
            current_type = Some(name);
            continue;
        }

        // Type alias / other type definitions
        if let Some(caps) = re_type_def().captures(trimmed) {
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name,
                kind: "interface".into(),
                summary: Some(trimmed.to_string()),
                line: Some(line_num),
                language: "go".into(),
                parent_name: None,
                metadata: None,
            });
            continue;
        }

        // Function / method (func keyword)
        if let Some(caps) = re_func().captures(trimmed) {
            let name = caps[1].to_string();
            let is_method = trimmed.starts_with("func") && trimmed.contains(')') && !trimmed.starts_with("func(");
            let mut meta = HashMap::new();
            if trimmed.contains("http.Handler") || name == "ServeHTTP" {
                meta.insert("http_handler".into(), "true".into());
            }

            symbols.push(ExtractedSymbol {
                name,
                kind: if is_method { "method" } else { "function" }.into(),
                summary: Some(extract_fn_sig(trimmed)),
                line: Some(line_num),
                language: "go".into(),
                parent_name: if is_method { current_type.clone() } else { None },
                metadata: Some(meta),
            });
        }
    }

    symbols
}

fn extract_fn_sig(line: &str) -> String {
    if let Some(pos) = line.find('{') {
        line[..pos].trim().to_string()
    } else {
        line.trim().trim_end_matches('{').trim().to_string()
    }
}

use std::sync::OnceLock;
use regex::Regex;

fn re_package() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^package\s+(\w+)").unwrap())
}

fn re_func() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"func\s+(?:\(\w+\s+\*?\w+\)\s+)?(\w+)").unwrap())
}

fn re_struct() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"type\s+(\w+)\s+struct").unwrap())
}

fn re_interface() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"type\s+(\w+)\s+interface").unwrap())
}

fn re_type_def() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"type\s+(\w+)\s+\w+").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_go_symbols() {
        let code = r#"package main

type Server struct {
    addr string
}

type Handler interface {
    ServeHTTP(w http.ResponseWriter, r *http.Request)
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {}

func NewServer(addr string) *Server {
    return &Server{addr: addr}
}
"#;
        let symbols = extract_symbols(code);
        assert!(symbols.iter().any(|s| s.name == "main" && s.kind == "module"));
        assert!(symbols.iter().any(|s| s.name == "Server" && s.kind == "struct"));
        assert!(symbols.iter().any(|s| s.name == "Handler" && s.kind == "interface"));
        assert!(symbols.iter().any(|s| s.name == "ServeHTTP" && s.kind == "method"));
        assert!(symbols.iter().any(|s| s.name == "NewServer" && s.kind == "function"));
    }
}
