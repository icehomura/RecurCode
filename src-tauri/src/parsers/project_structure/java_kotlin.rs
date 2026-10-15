use std::collections::HashMap;

use super::ExtractedSymbol;

/// Extract Java/Kotlin symbols: classes, methods, interfaces, enums.
pub fn extract_symbols(content: &str, language: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();

        // Track class body
        if current_class.is_some() {
            brace_depth += trimmed.matches('{').count() as i32;
            let close = trimmed.matches('}').count() as i32;
            brace_depth -= close;
            if brace_depth <= 0 {
                current_class = None;
                brace_depth = 0;
            }
        }

        // Class / data class (Kotlin)
        if let Some(caps) = re_class(language).captures(trimmed) {
            let name = caps[1].to_string();
            let mut meta = HashMap::new();
            if trimmed.contains("abstract") {
                meta.insert("abstract".into(), "true".into());
            }
            if trimmed.contains("data class") {
                meta.insert("data_class".into(), "true".into());
            }
            if trimmed.contains("open ") || trimmed.contains("public ") {
                meta.insert("visibility".into(), "public".into());
            }
            symbols.push(ExtractedSymbol {
                name: name.clone(),
                kind: "class".into(),
                summary: None,
                line: Some(line_num),
                language: language.into(),
                parent_name: None,
                metadata: Some(meta),
            });
            current_class = Some(name);
            // Count this line's own braces so a method that opens and closes on
            // one line (`void f() {}`) does not falsely return depth to 0 and end
            // the class scope.
            brace_depth = trimmed.matches('{').count() as i32
                - trimmed.matches('}').count() as i32;
            continue;
        }

        // Interface
        if let Some(caps) = re_interface(language).captures(trimmed) {
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name: name.clone(),
                kind: "interface".into(),
                summary: None,
                line: Some(line_num),
                language: language.into(),
                parent_name: None,
                metadata: None,
            });
            current_class = Some(name);
            brace_depth = trimmed.matches('{').count() as i32
                - trimmed.matches('}').count() as i32;
            continue;
        }

        // Enum
        if let Some(caps) = re_enum(language).captures(trimmed) {
            let name = caps[1].to_string();
            symbols.push(ExtractedSymbol {
                name,
                kind: "enum".into(),
                summary: None,
                line: Some(line_num),
                language: language.into(),
                parent_name: None,
                metadata: None,
            });
            continue;
        }

        // Function / method
        if let Some(caps) = re_fn(language).captures(trimmed) {
            let name = caps[1].to_string();
            let mut meta = HashMap::new();
            if trimmed.contains("override") {
                meta.insert("override".into(), "true".into());
            }
            if trimmed.contains("static") || trimmed.contains("companion") {
                meta.insert("static".into(), "true".into());
            }

            symbols.push(ExtractedSymbol {
                name,
                kind: if current_class.is_some() {
                    "method"
                } else {
                    "function"
                }
                .into(),
                summary: Some(extract_fn_sig(trimmed)),
                line: Some(line_num),
                language: language.into(),
                parent_name: current_class.clone(),
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
        line.trim().trim_end_matches(';').trim().to_string()
    }
}

use std::sync::OnceLock;
use regex::Regex;

fn re_class(lang: &str) -> &'static Regex {
    static RE_JAVA: OnceLock<Regex> = OnceLock::new();
    static RE_KT: OnceLock<Regex> = OnceLock::new();
    if lang == "kotlin" {
        RE_KT.get_or_init(|| {
            Regex::new(r"(?:data\s+)?(?:abstract\s+)?(?:open\s+)?class\s+(\w+)").unwrap()
        })
    } else {
        RE_JAVA.get_or_init(|| {
            Regex::new(r"(?:public|private|protected)?\s*(?:abstract\s+)?class\s+(\w+)").unwrap()
        })
    }
}

fn re_interface(lang: &str) -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    let _ = lang;
    RE.get_or_init(|| Regex::new(r"interface\s+(\w+)").unwrap())
}

fn re_enum(lang: &str) -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    let _ = lang;
    RE.get_or_init(|| Regex::new(r"enum\s+(\w+)").unwrap())
}

fn re_fn(lang: &str) -> &'static Regex {
    static RE_JAVA: OnceLock<Regex> = OnceLock::new();
    static RE_KT: OnceLock<Regex> = OnceLock::new();
    if lang == "kotlin" {
        RE_KT.get_or_init(|| Regex::new(r"fun\s+(\w+)").unwrap())
    } else {
        RE_JAVA.get_or_init(|| {
            Regex::new(
                r"(?:public|private|protected)?\s*(?:static\s+)?(?:\w+\s+)+(\w+)\s*\(",
            )
            .unwrap()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_java_symbols() {
        let code = r#"
public class Server {
    public void handleRequest() {}
    private int calculate(int x) { return x; }
}

interface Handler {
    void process();
}

enum Color { RED, GREEN }
"#;
        let symbols = extract_symbols(code, "java");
        assert!(symbols.iter().any(|s| s.name == "Server" && s.kind == "class"));
        assert!(symbols.iter().any(|s| s.name == "handleRequest" && s.kind == "method"));
        assert!(symbols.iter().any(|s| s.name == "Handler" && s.kind == "interface"));
        assert!(symbols.iter().any(|s| s.name == "Color" && s.kind == "enum"));
    }

    #[test]
    fn extract_kotlin_symbols() {
        let code = r#"
data class Config(val name: String)

class Server {
    fun start() {}
    override fun toString(): String = "Server"
}

interface Repository {
    fun findAll(): List<Item>
}
"#;
        let symbols = extract_symbols(code, "kotlin");
        assert!(symbols.iter().any(|s| s.name == "Config" && s.kind == "class"));
        assert!(symbols.iter().any(|s| s.name == "Server" && s.kind == "class"));
        assert!(symbols.iter().any(|s| s.name == "start" && s.kind == "method"));
        assert!(symbols.iter().any(|s| s.name == "Repository" && s.kind == "interface"));
    }
}
