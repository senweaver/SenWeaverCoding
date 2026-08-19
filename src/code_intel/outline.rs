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

pub const MAX_OUTLINE_SOURCE_BYTES: u64 = 2 * 1024 * 1024;

pub fn extract_outline(
    path: &Path,
    language: Option<&str>,
) -> Result<Vec<OutlineEntry>, OutlineError> {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_OUTLINE_SOURCE_BYTES {
            tracing::debug!(
                target: "code_intel.outline",
                path = %path.display(),
                bytes = meta.len(),
                "skipping outline for oversized file (exceeds cap)"
            );
            return Ok(Vec::new());
        }
    }
    let source = std::fs::read_to_string(path).map_err(|e| OutlineError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    extract_outline_from_source(path, &source, language)
}

pub fn extract_outline_from_source(
    path: &Path,
    source: &str,
    language: Option<&str>,
) -> Result<Vec<OutlineEntry>, OutlineError> {
    let lang = language
        .map(str::to_string)
        .or_else(|| infer_language(path))
        .ok_or_else(|| OutlineError::UnsupportedLanguage("<unknown>".into()))?;

    #[cfg(feature = "code-intel")]
    {
        if let Some(entries) = tree_sitter_outline(source, &lang) {
            if !entries.is_empty() {
                return Ok(entries);
            }
        }
    }

    Ok(heuristic_outline(source, &lang))
}

fn heuristic_outline(source: &str, lang: &str) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx as u32 + 1;
        let trimmed = raw.trim_start();
        match lang {
            "rust" => {
                let stripped = strip_rust_modifiers(trimmed);
                if let Some(rest) = strip_keyword(stripped, &["fn "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("function", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(stripped, &["struct "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("struct", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(stripped, &["enum "]) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("enum", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(stripped, &["trait "]) {
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
            "javascript" | "typescript" | "tsx" => {
                if let Some(rest) = strip_keyword(
                    trimmed,
                    &[
                        "export function ",
                        "function ",
                        "export async function ",
                        "async function ",
                        "export default function ",
                        "export default async function ",
                    ],
                ) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("function", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(
                    trimmed,
                    &["export class ", "export abstract class ", "abstract class ", "class "],
                ) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("class", name, line_no));
                    }
                } else if let Some(rest) =
                    strip_keyword(trimmed, &["export interface ", "interface "])
                {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("interface", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(
                    trimmed,
                    &["export enum ", "export const enum ", "const enum ", "enum "],
                ) {
                    if let Some(name) = take_identifier(rest) {
                        out.push(entry("enum", name, line_no));
                    }
                } else if let Some(rest) = strip_keyword(
                    trimmed,
                    &["export const ", "export let ", "const ", "let ", "var "],
                ) {
                    if let Some(name) = take_identifier(rest) {
                        let after = &rest[name.len()..];
                        if is_arrow_or_function_binding(after) {
                            out.push(entry("function", name, line_no));
                        }
                    }
                }
            }
            "go" => {
                if let Some(rest) = strip_keyword(trimmed, &["func "]) {
                    let rest = if rest.starts_with('(') {
                        rest.find(')')
                            .map(|i| rest[i + 1..].trim_start())
                            .unwrap_or(rest)
                    } else {
                        rest
                    };
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

fn strip_rust_modifiers(mut s: &str) -> &str {
    loop {
        if let Some(rest) = s.strip_prefix("pub(") {
            if let Some(close) = rest.find(')') {
                s = rest[close + 1..].trim_start();
                continue;
            }
        }
        let mut changed = false;
        for m in ["pub ", "async ", "unsafe ", "const ", "extern \"C\" ", "extern "] {
            if let Some(rest) = s.strip_prefix(m) {
                s = rest.trim_start();
                changed = true;
            }
        }
        if !changed {
            return s;
        }
    }
}

fn is_arrow_or_function_binding(after_name: &str) -> bool {
    let rest = after_name.trim_start();
    let rest = match rest.strip_prefix(':') {
        Some(r) => match r.find('=') {
            Some(i) => &r[i..],
            None => return false,
        },
        None => rest,
    };
    let Some(rhs) = rest.strip_prefix('=') else {
        return false;
    };
    let rhs = rhs.trim_start();
    let rhs = rhs.strip_prefix("async ").map(str::trim_start).unwrap_or(rhs);
    rhs.starts_with("function") || rhs.contains("=>")
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
    locate_named_scope_in(&content, scope_name)
}

pub fn locate_named_scope_in(
    content: &str,
    scope_name: &str,
) -> Option<std::ops::Range<usize>> {
    let name = normalize_scope_name(scope_name);
    if name.is_empty() {
        return None;
    }

    for pattern in scope_search_patterns(&name) {
        if let Some(range) = find_named_scope_with_pattern(content, &pattern, &name) {
            return Some(range);
        }
    }
    None
}

fn normalize_scope_name(scope_name: &str) -> String {
    let mut s = scope_name.trim();
    loop {
        if let Some(rest) = s.strip_prefix("pub(") {
            if let Some(close) = rest.find(')') {
                s = rest[close + 1..].trim_start();
                continue;
            }
        }
        let mut changed = false;
        for m in [
            "pub ", "export ", "default ", "static ", "async ", "unsafe ", "const ",
            "private ", "public ", "protected ", "override ", "abstract ",
        ] {
            if let Some(rest) = s.strip_prefix(m) {
                s = rest.trim_start();
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for kw in [
        "fn ", "def ", "function ", "class ", "func ", "let ", "var ",
    ] {
        if let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim_start();
            break;
        }
    }
    let s = s.trim();
    match s.find('(') {
        Some(0) | None => s.to_string(),
        Some(i) => s[..i].trim_end().to_string(),
    }
}

fn scope_search_patterns(name: &str) -> Vec<String> {
    vec![
        format!("fn {name}"),
        format!("async fn {name}"),
        format!("def {name}"),
        format!("async def {name}"),
        format!("function {name}"),
        format!("async function {name}"),
        format!("function* {name}"),
        format!("class {name}"),
        format!("interface {name}"),
        format!("func {name}"),
        format!(") {name}("),
        format!("const {name} "),
        format!("const {name}="),
        format!("const {name}:"),
        format!("let {name} "),
        format!("let {name}="),
        format!("let {name}:"),
        format!("var {name} "),
        format!("var {name}="),
        format!("{name}("),
    ]
}

fn find_named_scope_with_pattern(
    content: &str,
    search: &str,
    name: &str,
) -> Option<std::ops::Range<usize>> {
    let bytes = content.as_bytes();
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bare_call_pattern = search == format!("{name}(");
    let leading_boundary_exempt = search.starts_with(") ");

    let mut search_from = 0usize;
    let start = loop {
        let rel = content[search_from..].find(search)?;
        let at = search_from + rel;
        let before_ok =
            leading_boundary_exempt || at == 0 || !is_ident_byte(bytes[at - 1]);
        if bare_call_pattern {
            let line_start = content[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix_ok = content[line_start..at].split_whitespace().all(|tok| {
                matches!(
                    tok,
                    "async"
                        | "static"
                        | "public"
                        | "private"
                        | "protected"
                        | "export"
                        | "override"
                        | "abstract"
                        | "readonly"
                        | "get"
                        | "set"
                        | "*"
                )
            });
            if !prefix_ok {
                search_from = at + 1;
                continue;
            }
        }
        let name_at = search
            .rfind(name)
            .map(|idx| at + idx)
            .unwrap_or(at);
        let after = name_at + name.len();
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if before_ok && after_ok {
            if search.starts_with("func ") {
                let after_func = at + "func ".len();
                if after_func < bytes.len() && bytes[after_func] == b'(' {
                    let mut probe_end = (at + search.len() + 64).min(content.len());
                    while probe_end > at && !content.is_char_boundary(probe_end) {
                        probe_end -= 1;
                    }
                    if !content[after_func..].contains(&format!(") {name}"))
                        && !content[at..probe_end].contains(name)
                    {
                        search_from = at + 1;
                        continue;
                    }
                }
            }
            break at;
        }
        search_from = at + 1;
    };

    if looks_like_python_def(content, start) {
        return python_indent_scope_end(content, start);
    }

    brace_scope_end(content, start)
}

fn looks_like_python_def(content: &str, start: usize) -> bool {
    let line_start = content[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = content[line_start..].lines().next().unwrap_or("");
    let trimmed = line.trim_start();
    trimmed.starts_with("def ")
        || trimmed.starts_with("async def ")
        || trimmed.starts_with("class ")
}

fn python_indent_scope_end(content: &str, start: usize) -> Option<std::ops::Range<usize>> {
    let line_start = content[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let first_line = content[line_start..].lines().next()?;
    let base_indent = first_line.len() - first_line.trim_start().len();
    let mut offset = line_start + first_line.len();
    if content.as_bytes().get(offset) == Some(&b'\n') {
        offset += 1;
    } else if content.get(offset..offset + 2) == Some("\r\n") {
        offset += 2;
    }
    let mut end = offset;
    while offset < content.len() {
        let rest = &content[offset..];
        let line = rest.lines().next().unwrap_or("");
        let line_len = line.len();
        let next = offset + line_len;
        let nl = if content.get(next..next + 2) == Some("\r\n") {
            2
        } else if content.as_bytes().get(next) == Some(&b'\n') {
            1
        } else {
            0
        };
        if line.trim().is_empty() {
            end = next + nl;
            offset = end;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= base_indent {
            break;
        }
        end = next + nl;
        offset = end;
    }
    if end > start {
        Some(start..end)
    } else {
        None
    }
}

fn brace_scope_end(content: &str, start: usize) -> Option<std::ops::Range<usize>> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut end = start;
    let mut i = start;
    let mut saw_open = false;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => break,
                        _ => i += 1,
                    }
                }
            }
            b'\'' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                    i += 3;
                } else if i + 2 < bytes.len() && bytes[i + 2] == b'\'' {
                    i += 2;
                }
            }
            b'`' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'`' => break,
                        _ => i += 1,
                    }
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'{' => {
                depth += 1;
                saw_open = true;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && saw_open {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
        i += 1;
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
            "ts" => "typescript",
            "tsx" => "tsx",
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
            r"
                (function_item name: (identifier) @name) @fn
                (struct_item name: (type_identifier) @name) @st
                (enum_item name: (type_identifier) @name) @en
                (trait_item name: (type_identifier) @name) @tr
            "
        }
        "python" => {
            r"
                (function_definition name: (identifier) @name) @fn
                (class_definition name: (identifier) @name) @cls
            "
        }
        "javascript" => {
            r"
                (function_declaration name: (identifier) @name) @fn
                (class_declaration name: (identifier) @name) @cls
                (variable_declarator name: (identifier) @name value: (arrow_function)) @fn
                (method_definition name: (property_identifier) @name) @fn
            "
        }
        "typescript" | "tsx" => {
            r"
                (function_declaration name: (identifier) @name) @fn
                (class_declaration name: (type_identifier) @name) @cls
                (variable_declarator name: (identifier) @name value: (arrow_function)) @fn
                (method_definition name: (property_identifier) @name) @fn
                (interface_declaration name: (type_identifier) @name) @tr
                (enum_declaration name: (identifier) @name) @en
                (type_alias_declaration name: (type_identifier) @name) @tr
            "
        }

        #[cfg(feature = "code-intel-go")]
        "go" => {
            r"
                (function_declaration name: (identifier) @name) @fn
                (method_declaration name: (field_identifier) @name) @fn
                (type_declaration (type_spec name: (type_identifier) @name)) @st
            "
        }
        #[cfg(feature = "code-intel-java")]
        "java" => {
            r"
                (method_declaration name: (identifier) @name) @fn
                (class_declaration name: (identifier) @name) @cls
                (interface_declaration name: (identifier) @name) @tr
            "
        }
        #[cfg(feature = "code-intel-c")]
        "c" => {
            r"
                (function_definition declarator: (function_declarator declarator: (identifier) @name)) @fn
                (struct_specifier name: (type_identifier) @name) @st
                (enum_specifier name: (type_identifier) @name) @en
            "
        }
        #[cfg(feature = "code-intel-cpp")]
        "cpp" | "c++" => {
            r"
                (function_definition declarator: (function_declarator declarator: (identifier) @name)) @fn
                (function_definition declarator: (function_declarator declarator: (qualified_identifier) @name)) @fn
                (class_specifier name: (type_identifier) @name) @cls
                (struct_specifier name: (type_identifier) @name) @st
                (enum_specifier name: (type_identifier) @name) @en
            "
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
