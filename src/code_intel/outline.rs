// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutlineEntry {

    pub kind: String,

    pub name: String,

    pub line: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum OutlineError {
    #[error("cannot read source file '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("language '{0}' is not supported by the current build")]
    UnsupportedLanguage(String),
}

pub fn extract_outline(
    path: &Path,
    language: Option<&str>,
) -> Result<Vec<OutlineEntry>, OutlineError> {
    let source = std::fs::read_to_string(path).map_err(|e| OutlineError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let lang = language
        .map(str::to_string)
        .or_else(|| infer_language(path))
        .ok_or_else(|| OutlineError::UnsupportedLanguage("<unknown>".into()))?;

    #[cfg(feature = "code-intel")]
    {
        if let Some(entries) = tree_sitter_outline(&source, &lang) {
            return Ok(entries);
        }
    }

    Ok(heuristic_outline(&source, &lang))
}

fn heuristic_outline(source: &str, lang: &str) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx as u32 + 1;
        let trimmed = raw.trim_start();
        match lang {
            "rust" => {
                if let Some(rest) =
                    strip_keyword(trimmed, &["pub fn ", "fn ", "pub async fn ", "async fn "])
                {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("function", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(trimmed, &["pub struct ", "struct "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("struct", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(trimmed, &["pub enum ", "enum "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("enum", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(trimmed, &["pub trait ", "trait "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("trait", name, line_no));
                    }
                }
            }
            "python" => {
                if let Some(rest) = strip_keyword(trimmed, &["def ", "async def "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("function", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(trimmed, &["class "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("class", name, line_no));
                    }
                }
            }
            "javascript" | "typescript" => {
                if let Some(rest) = strip_keyword(
                    trimmed,
                    &[
                        "export function ",
                        "function ",
                        "export async function ",
                        "async function ",
                    ],
                ) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("function", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(trimmed, &["export class ", "class "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("class", name, line_no));
                    }
                }
            }
            "go" => {
                if let Some(rest) = strip_keyword(trimmed, &["func "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("function", name, line_no));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn strip_keyword<'a>(s: &'a str, keywords: &[&str]) -> Option<&'a str> {
    for k in keywords {
        if let Some(rest) = s.strip_prefix(k) {
            return Some(rest);
        }
    }
    None
}

fn take_identifier(s: &str) -> Option<String> {
    let mut end = 0usize;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        None
    } else {
        Some(s[..end].to_string())
    }
}

fn entry(kind: &str, name: String, line: u32) -> OutlineEntry {
    OutlineEntry {
        kind: kind.to_string(),
        name,
        line,
    }
}

pub fn locate_scope(path: &Path, line: u32) -> Option<String> {
    let entries = extract_outline(path, None).ok()?;
    entries
        .into_iter()
        .filter(|e| e.line <= line && e.kind == "function")
        .max_by_key(|e| e.line)
        .map(|e| e.name)
}

pub fn locate_named_scope(
    path: &std::path::Path,
    scope_name: &str,
) -> Option<std::ops::Range<usize>> {
    let content = std::fs::read_to_string(path).ok()?;
    let search = format!("fn {}", scope_name.trim_start_matches("fn "));
    let start = content.find(&search)?;

    let mut depth = 0usize;
    let mut end = start;
    let bytes = content.as_bytes();
    for (i, b) in bytes.iter().enumerate().skip(start) {
        match *b {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && i > start {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if end > start {
        Some(start..end)
    } else {
        None
    }
}

fn infer_language(path: &Path) -> Option<String> {
    let ext = path.extension().and_then(|s| s.to_str())?;
    Some(
        match ext.to_ascii_lowercase().as_str() {
            "rs" => "rust",
            "py" | "pyi" => "python",
            "js" | "mjs" | "cjs" | "jsx" => "javascript",
            "ts" | "tsx" => "typescript",
            "go" => "go",

            "java" => "java",
            "c" | "h" => "c",
            "cpp" | "cxx" | "cc" | "hpp" | "hh" | "hxx" => "cpp",
            _ => return None,
        }
        .to_string(),
    )
}

#[cfg(feature = "code-intel")]
fn tree_sitter_outline(source: &str, lang: &str) -> Option<Vec<OutlineEntry>> {
    use streaming_iterator::StreamingIterator;
    use tree_sitter::{Parser, QueryCursor};

    let language: tree_sitter::Language = super::grammars::grammar_for(lang)?;

    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;

    let query_src = match lang {
        "rust" => {
            r#"
                (function_item name: (identifier) @name) @fn
                (struct_item name: (type_identifier) @name) @st
                (enum_item name: (type_identifier) @name) @en
                (trait_item name: (type_identifier) @name) @tr
            "#
        }
        "python" => {
            r#"
                (function_definition name: (identifier) @name) @fn
                (class_definition name: (identifier) @name) @cls
            "#
        }
        "javascript" | "typescript" => {
            r#"
                (function_declaration name: (identifier) @name) @fn
                (class_declaration name: (identifier) @name) @cls
            "#
        }

        #[cfg(feature = "code-intel-go")]
        "go" => {
            r#"
                (function_declaration name: (identifier) @name) @fn
                (method_declaration name: (field_identifier) @name) @fn
                (type_declaration (type_spec name: (type_identifier) @name)) @st
            "#
        }
        #[cfg(feature = "code-intel-java")]
        "java" => {
            r#"
                (method_declaration name: (identifier) @name) @fn
                (class_declaration name: (identifier) @name) @cls
                (interface_declaration name: (identifier) @name) @tr
            "#
        }
        #[cfg(feature = "code-intel-c")]
        "c" => {
            r#"
                (function_definition declarator: (function_declarator declarator: (identifier) @name)) @fn
                (struct_specifier name: (type_identifier) @name) @st
                (enum_specifier name: (type_identifier) @name) @en
            "#
        }
        #[cfg(feature = "code-intel-cpp")]
        "cpp" | "c++" => {
            r#"
                (function_definition declarator: (function_declarator declarator: (identifier) @name)) @fn
                (function_definition declarator: (function_declarator declarator: (qualified_identifier) @name)) @fn
                (class_specifier name: (type_identifier) @name) @cls
                (struct_specifier name: (type_identifier) @name) @st
                (enum_specifier name: (type_identifier) @name) @en
            "#
        }
        _ => return None,
    };
    let query = tree_sitter::Query::new(&language, query_src).ok()?;
    let mut cursor = QueryCursor::new();
    let mut results: Vec<OutlineEntry> = Vec::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(m) = matches.next() {

        let mut kind: Option<&str> = None;
        let mut name_text: Option<String> = None;
        let mut start_line: u32 = 0;
        for cap in m.captures {
            let node = cap.node;
            let capture_name = &query.capture_names()[cap.index as usize];
            match *capture_name {
                "fn" => {
                    kind = Some("function");
                    start_line = node.start_position().row as u32 + 1;
                }
                "st" => {
                    kind = Some("struct");
                    start_line = node.start_position().row as u32 + 1;
                }
                "en" => {
                    kind = Some("enum");
                    start_line = node.start_position().row as u32 + 1;
                }
                "tr" => {
                    kind = Some("trait");
                    start_line = node.start_position().row as u32 + 1;
                }
                "cls" => {
                    kind = Some("class");
                    start_line = node.start_position().row as u32 + 1;
                }
                "name" => {
                    if let Ok(t) = node.utf8_text(source.as_bytes()) {
                        name_text = Some(t.to_string());
                    }
                }
                _ => {}
            }
        }
        if let (Some(k), Some(n)) = (kind, name_text) {
            results.push(OutlineEntry {
                kind: k.to_string(),
                name: n,
                line: start_line.max(1),
            });
        }
    }
    results.sort_by_key(|e| e.line);
    Some(results)
}
