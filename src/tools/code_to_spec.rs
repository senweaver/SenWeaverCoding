// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! `code_to_spec` tool — Code Structure Analysis and Specification Generation.
//!
//! Analyzes code structure and generates or updates structured specifications.
//! Part of the Harness engineering-grade workflow (Layer 1: Spec Layer).
//!
//! Actions:
//! - `analyze`: Extract structural information from files (functions, types, interfaces)
//! - `generate`: Generate a SPEC.md from the analysis
//! - `compare`: Compare current code against a SPEC.md and report gaps
//! - `summarize`: Generate a lightweight spec summary of the current state

use crate::tools::traits::{Tool, ToolResult};
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecSection {
    pub heading: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub language: String,
    pub structures: Vec<CodeStructure>,
    pub dependencies: Vec<String>,
    pub entry_points: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodeStructure {
    Function {
        name: String,
        signature: String,
        location: String,
        doc_comment: Option<String>,
    },
    Struct {
        name: String,
        fields: Vec<String>,
        location: String,
        doc_comment: Option<String>,
    },
    Enum {
        name: String,
        variants: Vec<String>,
        location: String,
        doc_comment: Option<String>,
    },
    Trait {
        name: String,
        methods: Vec<String>,
        location: String,
        doc_comment: Option<String>,
    },
    Module {
        name: String,
        location: String,
    },
    Interface {
        name: String,
        methods: Vec<String>,
        location: String,
    },
    Class {
        name: String,
        methods: Vec<String>,
        fields: Vec<String>,
        location: String,
        doc_comment: Option<String>,
    },
}

impl CodeStructure {
    fn format_markdown(&self) -> String {
        match self {
            Self::Function { name, signature, location, doc_comment } => {
                let doc = doc_comment.as_ref().map(|d| format!("\n  > {d}")).unwrap_or_default();
                format!("- **fn {}** `{}` at `{}`{}", name, signature, location, doc)
            }
            Self::Struct { name, fields, location, doc_comment } => {
                let doc = doc_comment.as_ref().map(|d| format!("\n  > {d}")).unwrap_or_default();
                let fields_str = fields.iter().map(|f| format!("  - {}", f)).collect::<Vec<_>>().join("\n");
                format!("- **struct {}** at `{}`{}\n{}", location, name, doc, fields_str)
            }
            Self::Enum { name, variants, location, doc_comment } => {
                let doc = doc_comment.as_ref().map(|d| format!("\n  > {d}")).unwrap_or_default();
                let variants_str = variants.iter().map(|v| format!("  - {}", v)).collect::<Vec<_>>().join("\n");
                format!("- **enum {}** at `{}`{}\n{}", location, name, doc, variants_str)
            }
            Self::Trait { name, methods, location, doc_comment } => {
                let doc = doc_comment.as_ref().map(|d| format!("\n  > {d}")).unwrap_or_default();
                let methods_str = methods.iter().map(|m| format!("  - {}", m)).collect::<Vec<_>>().join("\n");
                format!("- **trait {}** at `{}`{}\n{}", location, name, doc, methods_str)
            }
            Self::Module { name, location } => {
                format!("- **mod {}** at `{}`", name, location)
            }
            Self::Interface { name, methods, location } => {
                let methods_str = methods.iter().map(|m| format!("  - {}", m)).collect::<Vec<_>>().join("\n");
                format!("- **interface {}** at `{}`\n{}", location, name, methods_str)
            }
            Self::Class { name, methods, fields, location, doc_comment } => {
                let doc = doc_comment.as_ref().map(|d| format!("\n  > {d}")).unwrap_or_default();
                let methods_str = methods.iter().map(|m| format!("  - {}", m)).collect::<Vec<_>>().join("\n");
                let fields_str = fields.iter().map(|f| format!("  - {}", f)).collect::<Vec<_>>().join("\n");
                format!("- **class {}** at `{}`{}\n{}\n{}", location, name, doc, methods_str, fields_str)
            }
        }
    }
}

/// Heuristic language detection from file extension.
fn detect_language(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust".to_string(),
        Some("ts") | Some("tsx") => "typescript".to_string(),
        Some("js") | Some("jsx") | Some("mjs") => "javascript".to_string(),
        Some("py") => "python".to_string(),
        Some("go") => "go".to_string(),
        Some("java") => "java".to_string(),
        Some("cpp") | Some("cc") | Some("cxx") => "cpp".to_string(),
        Some("c") => "c".to_string(),
        Some("cs") => "csharp".to_string(),
        Some("rb") => "ruby".to_string(),
        Some("php") => "php".to_string(),
        Some("swift") => "swift".to_string(),
        Some("kt") | Some("kts") => "kotlin".to_string(),
        Some("scala") => "scala".to_string(),
        Some("md") => "markdown".to_string(),
        Some("json") => "json".to_string(),
        Some("yaml") | Some("yml") => "yaml".to_string(),
        Some("toml") => "toml".to_string(),
        _ => "unknown".to_string(),
    }
}

/// Find the first occurrence of any of the given chars in the string.
fn find_first_of(s: &str, chars: &[char]) -> Option<usize> {
    chars.iter().filter_map(|c| s.find(*c)).min()
}

/// Heuristically extract structures from Rust source code.
fn extract_rust_structures(content: &str, file_path: &str) -> Vec<CodeStructure> {
    let mut structures = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // Doc comment (skip)
        if line.starts_with("//!") || line.starts_with("///") {
            i += 1;
            continue;
        }

        // pub fn / fn
        if line.starts_with("pub fn ") || (line.starts_with("fn ") && !line.contains(" impl ")) {
            if let Some(name_end) = line.find('(') {
                let name_start = if line.starts_with("pub fn ") { 7 } else { 3 };
                let name = line[name_start..name_end].trim().to_string();
                let sig_start = name_end;
                let sig_end = line.find('{').unwrap_or(line.len());
                let signature = line[sig_start..sig_end].trim().to_string();
                let doc = extract_doc_comment_backward(&lines, i);
                structures.push(CodeStructure::Function {
                    name,
                    signature,
                    location: format!("{file_path}:{}", i + 1),
                    doc_comment: doc,
                });
            }
        }

        // struct — extract fields from following lines until closing brace or new declaration
        if line.starts_with("pub struct ") || line.starts_with("struct ") {
            if let Some(name_end) = find_first_of(line, &['(', '{', ';']) {
                let name_start = if line.starts_with("pub struct ") { 11 } else { 7 };
                let name = line[name_start..name_end].trim().to_string();
                let doc = extract_doc_comment_backward(&lines, i);

                // Fields appear on subsequent lines within the struct body
                let mut fields = Vec::new();
                let brace_count = line.matches('{').count() - line.matches('}').count();
                let mut j = i + 1;
                let mut local_braces = brace_count;
                while j < lines.len() && local_braces > 0 {
                    let field_line = lines[j].trim();
                    // Skip empty lines, comments, and doc comments
                    if field_line.is_empty()
                        || field_line.starts_with("//")
                        || field_line.starts_with("/*")
                        || field_line.starts_with("*/")
                    {
                        j += 1;
                        continue;
                    }
                    // Stop if we hit another top-level declaration
                    if !field_line.starts_with("    ")
                        && !field_line.starts_with('\t')
                        && !field_line.contains(':')
                        && !field_line.ends_with(',')
                        && !field_line.ends_with(';')
                        && !field_line.ends_with('}')
                    {
                        break;
                    }
                    // Try to extract field name and type
                    if let Some(colon_pos) = field_line.find(':') {
                        let field_name = field_line[..colon_pos]
                            .trim()
                            .trim_end_matches(',')
                            .trim_end_matches(';')
                            .to_string();
                        let rest = field_line[colon_pos + 1..].trim();
                        if !field_name.is_empty()
                            && !field_name.starts_with("pub ")
                            && !field_name.starts_with("pub(")
                            && !field_name.starts_with('_')
                            && !field_name.starts_with("//")
                        {
                            fields.push(format!("{}: {}", field_name, rest.trim_end_matches(',').trim_end_matches(';')));
                        }
                    } else if field_line.ends_with(',') || field_line.ends_with(';') {
                        // Tuple struct field: `field: Type,` — already captured above
                    }
                    local_braces += field_line.matches('{').count();
                    local_braces -= field_line.matches('}').count();
                    j += 1;
                }

                structures.push(CodeStructure::Struct {
                    name,
                    fields,
                    location: format!("{file_path}:{}", i + 1),
                    doc_comment: doc,
                });
            }
        }

        // enum — extract variants from following lines
        if line.starts_with("pub enum ") || line.starts_with("enum ") {
            if let Some(name_end) = find_first_of(line, &['{', ';']) {
                let name_start = if line.starts_with("pub enum ") { 9 } else { 5 };
                let name = line[name_start..name_end].trim().to_string();
                let doc = extract_doc_comment_backward(&lines, i);

                let mut variants = Vec::new();
                let mut j = i + 1;
                // Count total braces to know when we've exited the enum body
                let total_open = line.matches('{').count();
                let total_close = line.matches('}').count();
                let mut brace_depth = total_open.saturating_sub(total_close);
                while j < lines.len() {
                    let vline = lines[j].trim();
                    brace_depth += vline.matches('{').count();
                    brace_depth -= vline.matches('}').count();
                    // Only process content while inside the enum body
                    if brace_depth > 0
                        && !vline.is_empty()
                        && !vline.starts_with("//")
                        && !vline.starts_with("/*")
                        && !vline.starts_with("*/")
                    {
                        let variant_name = vline
                            .split(|c: char| c == '(' || c == '{' || c == ',' || c == '=' || c == '}')
                            .next()
                            .unwrap_or(vline)
                            .trim()
                            .to_string();
                        if !variant_name.is_empty() && !variant_name.starts_with("//") {
                            variants.push(variant_name);
                        }
                    }
                    // Stop once we've exited the enum body (brace_depth <= 0 means we processed the closing brace)
                    if brace_depth <= 0 {
                        break;
                    }
                    j += 1;
                }

                structures.push(CodeStructure::Enum {
                    name,
                    variants,
                    location: format!("{file_path}:{}", i + 1),
                    doc_comment: doc,
                });
            }
        }

        // trait — extract method signatures from the trait body
        if line.starts_with("pub trait ") || line.starts_with("trait ") {
            if let Some(name_end) = find_first_of(line, &['{', ':']) {
                let name_start = if line.starts_with("pub trait ") { 10 } else { 6 };
                let name = line[name_start..name_end].trim().to_string();
                let doc = extract_doc_comment_backward(&lines, i);

                let mut methods = Vec::new();
                let mut j = i + 1;
                let total_open = line.matches('{').count();
                let total_close = line.matches('}').count();
                let mut brace_depth = total_open.saturating_sub(total_close);
                while j < lines.len() {
                    let mline = lines[j].trim();
                    brace_depth += mline.matches('{').count();
                    brace_depth -= mline.matches('}').count();
                    if brace_depth > 0
                        && !mline.is_empty()
                        && !mline.starts_with("//")
                        && !mline.starts_with("/*")
                        && !mline.starts_with("*/")
                    {
                        if mline.starts_with("fn ")
                            || mline.starts_with("pub fn ")
                            || mline.starts_with("async fn ")
                            || mline.starts_with("pub async fn ")
                        {
                            let fn_sig = if let Some(paren_pos) = mline.find('(') {
                                let fn_name_start = if mline.starts_with("pub fn ") {
                                    7
                                } else if mline.starts_with("pub async fn ") {
                                    13
                                } else if mline.starts_with("async fn ") {
                                    9
                                } else {
                                    3
                                };
                                let name_part = &mline[fn_name_start..paren_pos].trim();
                                let params_and_return = mline[paren_pos..]
                                    .trim_end_matches('{')
                                    .trim_end_matches(';')
                                    .trim();
                                format!("{name_part}{params_and_return}")
                            } else {
                                mline.trim_end_matches('{').trim_end_matches(';').to_string()
                            };
                            methods.push(fn_sig);
                        }
                    }
                    if brace_depth <= 0 {
                        break;
                    }
                    j += 1;
                }

                structures.push(CodeStructure::Trait {
                    name,
                    methods,
                    location: format!("{file_path}:{}", i + 1),
                    doc_comment: doc,
                });
            }
        }

        // mod
        if line.starts_with("pub mod ") || line.starts_with("mod ") {
            if let Some(name_end) = find_first_of(line, &[';', '{']) {
                let name_start = if line.starts_with("pub mod ") { 8 } else { 4 };
                let name = line[name_start..name_end].trim().to_string();
                structures.push(CodeStructure::Module {
                    name,
                    location: format!("{file_path}:{}", i + 1),
                });
            }
        }

        i += 1;
    }

    structures
}

fn line_start(i: usize, _lines: &[&str]) -> usize {
    // Simplified: return 1-indexed line number
    i + 1
}

/// Extract doc comment(s) from lines immediately preceding line `i`.
/// Looks backward for `///`, `//!`, or `/**` comment blocks.
fn extract_doc_comment_backward(lines: &[&str], i: usize) -> Option<String> {
    if i == 0 {
        return None;
    }
    let mut collected: Vec<String> = Vec::new();
    let mut j = i;
    while j > 0 {
        j -= 1;
        let prev = lines[j].trim();
        if prev.starts_with("///") {
            collected.push(prev[3..].trim().to_string());
        } else if prev.starts_with("//!") {
            collected.push(prev[3..].trim().to_string());
        } else if prev.starts_with("/*") || prev.starts_with("*") {
            // Block comment — strip leading `/*`, `*`, and trailing `*/`
            let inner = prev
                .trim_start_matches("/*")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim();
            if !inner.is_empty() {
                collected.push(inner.to_string());
            }
        } else {
            // Non-comment line — stop here
            break;
        }
    }
    if collected.is_empty() {
        return None;
    }
    // Reverse so comments appear in natural top-to-bottom order
    collected.reverse();
    Some(collected.join(" ").trim().to_string())
}

/// Heuristically extract structures from TypeScript/JavaScript.
fn extract_ts_structures(content: &str, file_path: &str) -> Vec<CodeStructure> {
    let mut structures = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // Skip comments
        if line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") {
            i += 1;
            continue;
        }

        // function declaration
        if line.starts_with("export function ")
            || line.starts_with("function ")
            || line.starts_with("async function ")
        {
            let prefix_len = if line.starts_with("export function ") {
                15
            } else if line.starts_with("async function ") {
                15
            } else {
                8
            };
            if let Some(paren) = line[prefix_len..].find('(') {
                let name = line[prefix_len..prefix_len + paren].trim().to_string();
                let paren_end = line.find(')').unwrap_or(line.len());
                let signature = line[paren..=paren_end].to_string();
                structures.push(CodeStructure::Function {
                    name,
                    signature,
                    location: format!("{file_path}:{i}"),
                    doc_comment: None,
                });
            }
        }

        // class
        if line.starts_with("export class ")
            || line.starts_with("class ")
        {
            let prefix_len = if line.starts_with("export class ") { 13 } else { 6 };
            let rest = &line[prefix_len..];
            let name_end = rest.find([' ', '{', '(']).unwrap_or(rest.len());
            let name = rest[..name_end].trim().to_string();
            structures.push(CodeStructure::Class {
                name,
                methods: vec![],
                fields: vec![],
                location: format!("{file_path}:{i}"),
                doc_comment: None,
            });
        }

        // interface
        if line.starts_with("export interface ")
            || line.starts_with("interface ")
        {
            let prefix_len = if line.starts_with("export interface ") { 17 } else { 10 };
            if let Some(brace) = line[prefix_len..].find('{').or_else(|| line[prefix_len..].find(' ')) {
                let name = line[prefix_len..prefix_len + brace].trim().to_string();
                structures.push(CodeStructure::Interface {
                    name,
                    methods: vec![],
                    location: format!("{file_path}:{i}"),
                });
            }
        }

        i += 1;
    }

    structures
}

/// Heuristically extract structures from Python.
fn extract_python_structures(content: &str, file_path: &str) -> Vec<CodeStructure> {
    let mut structures = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // Skip comments and docstrings
        if line.starts_with('#') || line.starts_with("\"\"\"") || line.starts_with("'''") {
            i += 1;
            continue;
        }

        // class
        if line.starts_with("class ") || line.starts_with("async class ") {
            let prefix_len = if line.starts_with("async class ") { 12 } else { 6 };
            let rest = &line[prefix_len..];
            let name_end = rest.find([' ', ':']).unwrap_or(rest.len());
            let name = rest[..name_end].trim().to_string();
            structures.push(CodeStructure::Class {
                name,
                methods: vec![],
                fields: vec![],
                location: format!("{file_path}:{i}"),
                doc_comment: None,
            });
        }

        // def / async def
        if line.starts_with("def ") || line.starts_with("async def ") {
            let is_async = line.starts_with("async ");
            let content_part = if is_async { &line[10..] } else { &line[4..] };
            let name_end = content_part.find('(').unwrap_or(content_part.len());
            let name = content_part[..name_end].trim().to_string();
            let paren_end = content_part[name_end..].find(')').map(|p| name_end + p + 1).unwrap_or(line.len());
            let signature = content_part[name_end..paren_end].to_string();

            structures.push(CodeStructure::Function {
                name,
                signature,
                location: format!("{file_path}:{i}"),
                doc_comment: None,
            });
        }

        i += 1;
    }
    structures
}

/// Heuristically extract structures from Go.
fn extract_go_structures(content: &str, file_path: &str) -> Vec<CodeStructure> {
    let mut structures = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        if line.starts_with("//") {
            i += 1;
            continue;
        }

        // func (possibly with receiver)
        if line.starts_with("func ") {
            let body = &line[5..];
            let (name, sig) = if body.starts_with('(') {
                // method with receiver: func (r *Receiver) Name(args) returntype
                if let Some(close_paren) = body.find(')') {
                    let rest = &body[close_paren + 1..];
                    let name_end = rest.find('(').unwrap_or(rest.len());
                    let name = rest[..name_end].trim().to_string();
                    let sig_start = name_end;
                    let sig_end = rest[sig_start..].find('{').unwrap_or(rest.len());
                    (name, rest[sig_start..sig_start + sig_end].to_string())
                } else {
                    (String::new(), String::new())
                }
            } else {
                // top-level function
                let name_end = body.find('(').unwrap_or(body.len());
                let name = body[..name_end].trim().to_string();
                let paren_end = body[name_end..].find(')').map(|p| name_end + p + 1).unwrap_or(line.len());
                (name, body[name_end..paren_end].to_string())
            };

            if !name.is_empty() {
                structures.push(CodeStructure::Function {
                    name,
                    signature: sig,
                    location: format!("{file_path}:{i}"),
                    doc_comment: None,
                });
            }
        }

        // type ... struct
        if line.starts_with("type ") && line.contains("struct") {
            if let Some(name_end) = line.find("struct") {
                let name = line[5..name_end].trim().to_string();
                structures.push(CodeStructure::Struct {
                    name,
                    fields: vec![],
                    location: format!("{file_path}:{i}"),
                    doc_comment: None,
                });
            }
        }

        // type ... interface
        if line.starts_with("type ") && line.contains("interface") {
            if let Some(name_end) = line.find("interface") {
                let name = line[5..name_end].trim().to_string();
                structures.push(CodeStructure::Trait {
                    name,
                    methods: vec![],
                    location: format!("{file_path}:{i}"),
                    doc_comment: None,
                });
            }
        }

        i += 1;
    }

    structures
}

/// Dispatch structure extraction by language.
fn extract_structures(content: &str, file_path: &str, language: &str) -> Vec<CodeStructure> {
    match language {
        "rust" => extract_rust_structures(content, file_path),
        "typescript" | "javascript" => extract_ts_structures(content, file_path),
        "python" => extract_python_structures(content, file_path),
        "go" => extract_go_structures(content, file_path),
        _ => Vec::new(),
    }
}

/// Build a structured spec markdown document from analysis.
fn build_spec_markdown(
    _files: &HashMap<String, String>,
    analysis: &HashMap<String, AnalysisResult>,
    title: &str,
    description: &str,
) -> String {
    let mut md = String::new();

    md.push_str(&format!("# {title}\n\n"));
    md.push_str(&format!("> Auto-generated by `code_to_spec` tool. Last updated: {}\n\n", chrono_lite_now()));

    md.push_str("## Overview\n\n");
    md.push_str(&format!("{description}\n\n"));

    md.push_str("## Files\n\n");
    for (path, result) in analysis {
        md.push_str(&format!("### `{}` ({})\n\n", path, result.language));
        if !result.entry_points.is_empty() {
            md.push_str("**Entry Points:**\n");
            for ep in &result.entry_points {
                md.push_str(&format!("- `{}`\n", ep));
            }
            md.push('\n');
        }
        if !result.structures.is_empty() {
            md.push_str("**Structures:**\n");
            for s in &result.structures {
                md.push_str(&format!("{}\n", s.format_markdown()));
            }
            md.push('\n');
        }
    }

    md.push_str("---\n\n");
    md.push_str("## Usage\n\n");
    md.push_str("- [ ] Implementation matches this spec\n");
    md.push_str("- [ ] All entry points have tests\n");
    md.push_str("- [ ] All public interfaces are documented\n");
    md.push_str("- [ ] Dependencies are documented\n");

    md
}

fn chrono_lite_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("Unix timestamp: {}", now)
}

/// Compare a SPEC.md against actual file contents and report gaps.
fn compare_spec_with_code(
    spec_content: &str,
    files: &HashMap<String, String>,
    _language: &str,
) -> String {
    let mut gaps = Vec::new();

    // Extract headings from spec
    let spec_headings: Vec<&str> = spec_content
        .lines()
        .filter(|l| l.starts_with("## ") || l.starts_with("### "))
        .collect();

    // Heuristic: if spec mentions a struct/function but code doesn't have it, flag it
    for heading in &spec_headings {
        let name = heading.trim_start_matches('#').trim();
        let found = files.values().any(|c| c.contains(&format!(" {}", name)) || c.contains(&format!(" {}", name)));

        // Very simple heuristic check
        if !found && name.len() > 2 && !name.starts_with("Usage") && !name.starts_with("Files") {
            gaps.push(format!("- Spec mentions `{}` but not found in code", name));
        }
    }

    if gaps.is_empty() {
        "No significant gaps detected between spec and code. (Note: heuristic-based comparison; verify manually for accuracy.)".to_string()
    } else {
        format!("### Gaps Detected\n\n{}\n\n> This is a heuristic scan. Manual review recommended.", gaps.join("\n"))
    }
}

/// The Code-to-Spec tool.
pub struct CodeToSpecTool {
    workspace_dir: std::path::PathBuf,
}

impl CodeToSpecTool {
    pub fn new(workspace_dir: std::path::PathBuf) -> Self {
        Self { workspace_dir }
    }

    fn read_file(&self, rel_path: &str) -> anyhow::Result<String> {
        let full = self.workspace_dir.join(rel_path);
        std::fs::read_to_string(&full).with_context(|| format!("Failed to read {rel_path}"))
    }

    fn resolve_path(&self, path: &str) -> std::path::PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.workspace_dir.join(path)
        }
    }

    fn list_files_recursive(&self, dir: &Path, extensions: &[&str]) -> Vec<String> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Skip hidden dirs and common non-source dirs
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !name.starts_with('.') && name != "target" && name != "node_modules" && name != "__pycache__" {
                            files.extend(self.list_files_recursive(&path, extensions));
                        }
                    }
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if extensions.iter().any(|&e| e == ext) {
                        if let Ok(rel) = path.strip_prefix(&self.workspace_dir) {
                            files.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
        files
    }

    fn run_analysis(
        &self,
        files: &HashMap<String, String>,
    ) -> HashMap<String, AnalysisResult> {
        let mut analysis = HashMap::new();

        for (path, content) in files {
            let language = detect_language(Path::new(path));
            let structures = extract_structures(content, path, &language);

            // Heuristic entry point detection
            let mut entry_points = Vec::new();
            let name = Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if name == "main" || name == "lib" || name == "index" || name == "app" || name == "server" {
                entry_points.push(path.clone());
            }

            analysis.insert(
                path.clone(),
                AnalysisResult {
                    language,
                    structures,
                    dependencies: Vec::new(),
                    entry_points,
                },
            );
        }

        analysis
    }

    fn summarize_state(&self, files: &HashMap<String, String>, analysis: &HashMap<String, AnalysisResult>) -> String {
        let total_files = files.len();
        let total_structures: usize = analysis.values().map(|r| r.structures.len()).sum();

        let language_counts: HashMap<&str, usize> = analysis
            .values()
            .fold(HashMap::new(), |mut acc, r| {
                *acc.entry(r.language.as_str()).or_insert(0) += 1;
                acc
            });

        let languages: Vec<String> = language_counts
            .iter()
            .map(|(l, c)| format!("  - {} ({} files)", l, c))
            .collect();

        let struct_summary: Vec<String> = analysis
            .values()
            .flat_map(|r| r.structures.iter())
            .map(|s| match s {
                CodeStructure::Function { name, .. } => format!("- fn {}", name),
                CodeStructure::Struct { name, .. } => format!("- struct {}", name),
                CodeStructure::Enum { name, .. } => format!("- enum {}", name),
                CodeStructure::Trait { name, .. } => format!("- trait {}", name),
                CodeStructure::Module { name, .. } => format!("- mod {}", name),
                CodeStructure::Interface { name, .. } => format!("- interface {}", name),
                CodeStructure::Class { name, .. } => format!("- class {}", name),
            })
            .take(50) // Cap at 50 for brevity
            .collect();

        format!(
            "## Codebase Summary\n\n\
             - **Total files**: {total_files}\n\
             - **Total structures**: {total_structures}\n\
             - **Languages**:\n\
             {}\n\n\
             ## Top-Level Structures\n\n\
             {}\n\n\
             {}\
             {}\
             ",
            if languages.is_empty() { "  - (none detected)".to_string() } else { languages.join("\n") },
            if struct_summary.is_empty() { "  (none detected)".to_string() } else { struct_summary.join("\n") },
            if struct_summary.len() >= 50 { "\n _(truncated, see analysis for full list)_\n" } else { "" },
            if total_structures > 50 { "\n _(truncated, see analysis for full list)_\n" } else { "" }
        )
    }
}

#[async_trait]
impl Tool for CodeToSpecTool {
    fn name(&self) -> &str {
        "code_to_spec"
    }

    fn description(&self) -> &str {
        "Analyze code structure and generate/update structured specifications. \
         Use 'analyze' to extract structural information from files, \
         'generate' to produce a SPEC.md, \
         'compare' to check code against an existing spec, \
         'summarize' to get a quick codebase overview. \
         This is the first step of the Harness Spec Layer."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform: 'analyze' (extract structures), 'generate' (create SPEC.md), 'compare' (check gaps), 'summarize' (quick overview)",
                    "enum": ["analyze", "generate", "compare", "summarize"]
                },
                "paths": {
                    "type": "array",
                    "description": "File or directory paths to analyze (relative to workspace). For 'generate', this is where SPEC.md will be created.",
                    "items": { "type": "string" },
                    "default": ["."]
                },
                "title": {
                    "type": "string",
                    "description": "Title for the generated spec (used with 'generate' action)"
                },
                "description": {
                    "type": "string",
                    "description": "Brief description of the codebase (used with 'generate' action)"
                },
                "spec_path": {
                    "type": "string",
                    "description": "Path to an existing SPEC.md to compare against (used with 'compare' action)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        let paths: Vec<String> = args
            .get("paths")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_else(|| vec![".".to_string()]);

        match action {
            "analyze" => {
                let files = self.gather_files(&paths)?;
                let analysis = self.run_analysis(&files);

                let mut output_lines = Vec::new();
                output_lines.push(format!("## Analysis Results ({} files)\n", files.len()));

                for (path, result) in &analysis {
                    output_lines.push(format!("### `{}` ({})\n", path, result.language));
                    if !result.structures.is_empty() {
                        output_lines.push(format!("Found {} structures:\n", result.structures.len()));
                        for s in &result.structures {
                            output_lines.push(format!("{}\n", s.format_markdown()));
                        }
                    } else {
                        output_lines.push("No structures detected.\n".to_string());
                    }
                    output_lines.push("---\n\n".to_string());
                }

                Ok(ToolResult {
                    success: true,
                    output: output_lines.join(""),
                    error: None,
                })
            }

            "summarize" => {
                let files = self.gather_files(&paths)?;
                let analysis = self.run_analysis(&files);
                let summary = self.summarize_state(&files, &analysis);

                Ok(ToolResult {
                    success: true,
                    output: summary,
                    error: None,
                })
            }

            "generate" => {
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Project Specification")
                    .to_string();
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Auto-generated specification from code analysis.")
                    .to_string();

                let files = self.gather_files(&paths)?;
                let analysis = self.run_analysis(&files);
                let spec = build_spec_markdown(&files, &analysis, &title, &description);

                // Determine output path
                let output_path = if paths.len() == 1 && paths[0] != "." {
                    Path::new(&paths[0])
                        .file_name()
                        .map(|_n| "SPEC.md".to_string())
                        .unwrap_or_else(|| "SPEC.md".to_string())
                } else {
                    "SPEC.md".to_string()
                };

                let full_path = self.workspace_dir.join(&output_path);
                std::fs::write(&full_path, &spec)
                    .with_context(|| format!("Failed to write SPEC.md to {}", output_path))?;

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Generated specification at `{}` ({} files analyzed, {} total structures).\n\n\
                         Preview:\n\n{}",
                        output_path,
                        files.len(),
                        analysis.values().map(|r| r.structures.len()).sum::<usize>(),
                        &spec[..spec.len().min(2000)]
                    ),
                    error: None,
                })
            }

            "compare" => {
                let spec_path = args
                    .get("spec_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SPEC.md");

                let spec_content = self.read_file(spec_path).unwrap_or_else(|_| {
                    format!("[SPEC.md not found at {}]", spec_path)
                });
                let files = self.gather_files(&paths)?;
                let first_file = files.keys().next().map(|s| s.as_str()).unwrap_or("unknown");
                let language = detect_language(Path::new(first_file));

                let gaps = compare_spec_with_code(&spec_content, &files, &language);

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "## Spec vs Code Comparison\n\nComparing {} file(s) against `{}`:\n\n{}\n",
                        files.len(),
                        spec_path,
                        gaps
                    ),
                    error: None,
                })
            }

            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{other}'. Use: 'analyze', 'generate', 'compare', or 'summarize'."
                )),
            }),
        }
    }
}

impl CodeToSpecTool {
    fn gather_files(&self, paths: &[String]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();

        let code_extensions = ["rs", "ts", "tsx", "js", "jsx", "mjs", "py", "go", "java", "md", "toml", "yaml", "yml", "json"];

        for path in paths {
            let resolved = self.resolve_path(path);
            if resolved.is_file() {
                if let Ok(content) = std::fs::read_to_string(&resolved) {
                    if let Ok(rel) = resolved.strip_prefix(&self.workspace_dir) {
                        files.insert(rel.to_string_lossy().to_string(), content);
                    }
                }
            } else if resolved.is_dir() {
                for file in self.list_files_recursive(&resolved, &code_extensions) {
                    if let Ok(content) = self.read_file(&file) {
                        files.insert(file, content);
                    }
                }
            }
        }

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name() {
        let tool = CodeToSpecTool::new(std::path::PathBuf::from("."));
        assert_eq!(tool.name(), "code_to_spec");
    }

    #[test]
    fn detect_language_rust() {
        assert_eq!(detect_language(Path::new("src/main.rs")), "rust");
    }

    #[test]
    fn detect_language_typescript() {
        assert_eq!(detect_language(Path::new("src/app.ts")), "typescript");
        assert_eq!(detect_language(Path::new("src/app.tsx")), "typescript");
    }

    #[test]
    fn detect_language_python() {
        assert_eq!(detect_language(Path::new("main.py")), "python");
    }

    #[test]
    fn detect_language_go() {
        assert_eq!(detect_language(Path::new("server.go")), "go");
    }

    #[test]
    fn rust_function_extraction() {
        let code = r#"
/// Adds two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub struct Counter {
    count: i32,
}
"#;
        let structures = extract_rust_structures(code, "test.rs");
        assert!(!structures.is_empty());
        assert!(structures.iter().any(|s| match s {
            CodeStructure::Function { name, .. } => name == "add",
            _ => false,
        }));
    }

    #[test]
    fn ts_function_extraction() {
        let code = r#"
export function greet(name: string): string {
    return `Hello, ${name}`;
}

export class Greeter {
    private name: string;
}
"#;
        let structures = extract_ts_structures(code, "test.ts");
        assert!(!structures.is_empty());
        assert!(structures.iter().any(|s| match s {
            CodeStructure::Function { name, .. } => name == "greet",
            _ => false,
        }));
    }

    #[test]
    fn python_function_extraction() {
        let code = r#"
def hello(name: str) -> str:
    return f"Hello, {name}"

class Greeter:
    pass
"#;
        let structures = extract_python_structures(code, "test.py");
        assert!(!structures.is_empty());
    }

    #[test]
    fn go_function_extraction() {
        let code = r#"
package main

func add(a int, b int) int {
    return a + b
}
"#;
        let structures = extract_go_structures(code, "test.go");
        assert!(!structures.is_empty());
        assert!(structures.iter().any(|s| match s {
            CodeStructure::Function { name, .. } => name == "add",
            _ => false,
        }));
    }

    #[test]
    fn structure_markdown_format() {
        let func = CodeStructure::Function {
            name: "test".to_string(),
            signature: "(a: i32) -> i32".to_string(),
            location: "main.rs:1".to_string(),
            doc_comment: Some("Test function".to_string()),
        };
        let md = func.format_markdown();
        assert!(md.contains("fn test"));
        assert!(md.contains("Test function"));
    }
}
