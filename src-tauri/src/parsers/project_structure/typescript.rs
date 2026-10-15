use std::collections::HashMap;

use super::ExtractedSymbol;

/// Extract TypeScript/JavaScript symbols: functions, classes, methods,
/// interfaces, type aliases, exports, constants.
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

        // Class declaration
        if let Some(caps) = re_class().captures(trimmed) {
            let name = caps[1].to_string();
            let mut meta = HashMap::new();
            if trimmed.contains("export") {
                meta.insert("exported".into(), "true".into());
            }
            if trimmed.contains("abstract") {
                meta.insert("abstract".into(), "true".into());
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
            brace_depth = 0;
            continue;
        }

        // Interface (TS only)
        if language == "typescript" {
            if let Some(caps) = re_interface().captures(trimmed) {
                let name = caps[1].to_string();
                let mut meta = HashMap::new();
                if trimmed.contains("export") {
                    meta.insert("exported".into(), "true".into());
                }
                symbols.push(ExtractedSymbol {
                    name,
                    kind: "interface".into(),
                    summary: None,
                    line: Some(line_num),
                    language: language.into(),
                    parent_name: None,
                    metadata: Some(meta),
                });
                continue;
            }
        }

        // Type alias (TS only)
        if language == "typescript" {
            if let Some(caps) = re_type_alias().captures(trimmed) {
                let name = caps[1].to_string();
                symbols.push(ExtractedSymbol {
                    name,
                    kind: "interface".into(),
                    summary: Some(trimmed.to_string()),
                    line: Some(line_num),
                    language: language.into(),
                    parent_name: None,
                    metadata: None,
                });
                continue;
            }
        }

        // Function / arrow function / const function
        if let Some(caps) = re_fn().captures(trimmed) {
            let name = caps[1].to_string();
            let mut meta = HashMap::new();
            if trimmed.contains("export") {
                meta.insert("exported".into(), "true".into());
            }
            if trimmed.contains("async") {
                meta.insert("async".into(), "true".into());
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
                language: language.into(),
                parent_name: current_class.clone(),
                metadata: Some(meta),
            });
            continue;
        }

        // const / let / var (top-level exports)
        if let Some(caps) = re_const().captures(trimmed) {
            let name = caps[1].to_string();
            // `const x = function () {}` is a function, not a constant; the
            // original pattern used a lookahead (unsupported by the `regex`
            // crate) to exclude it, so guard in code instead.
            if !trimmed.contains("function") {
                if trimmed.contains("export")
                    || name
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false)
                {
                    let mut meta = HashMap::new();
                    if trimmed.contains("export") {
                        meta.insert("exported".into(), "true".into());
                    }
                    symbols.push(ExtractedSymbol {
                        name,
                        kind: "struct".into(), // Constants map to struct kind in our model
                        summary: None,
                        line: Some(line_num),
                        language: language.into(),
                        parent_name: None,
                        metadata: Some(meta),
                    });
                }
            }
            continue;
        }

        // Enum (TS only)
        if language == "typescript" {
            if let Some(caps) = re_enum().captures(trimmed) {
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
            }
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

use regex::Regex;
use std::sync::OnceLock;

fn re_class() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:export\s+)?(?:abstract\s+)?class\s+(\w+)").unwrap())
}

fn re_interface() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:export\s+)?interface\s+(\w+)").unwrap())
}

fn re_fn() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:export\s+)?(?:async\s+)?(?:function\s+(\w+)|(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s+)?\()",
        )
        .unwrap()
    })
}

fn re_const() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:export\s+)?(?:const|let|var)\s+(\w+)\s*[=:]").unwrap())
}

fn re_type_alias() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:export\s+)?type\s+(\w+)").unwrap())
}

fn re_enum() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:export\s+)?enum\s+(\w+)").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_ts_symbols() {
        let code = r#"
export interface Props {
    name: string;
}

export class Button {
    constructor(private label: string) {}
    render(): void {}
}

export function createApp(): void {}

type ID = string | number;

export const MAX_SIZE = 100;
"#;
        let symbols = extract_symbols(code, "typescript");
        assert!(symbols
            .iter()
            .any(|s| s.name == "Props" && s.kind == "interface"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Button" && s.kind == "class"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "createApp" && s.kind == "function"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "ID" && s.kind == "interface"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "MAX_SIZE" && s.kind == "struct"));
    }
}
