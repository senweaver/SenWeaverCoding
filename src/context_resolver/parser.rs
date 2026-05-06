// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Parse `@tag:value` tokens from user-typed text.
//!
//! The parser is intentionally tolerant: it recognises both
//! `@file:path/to/foo.rs` and `@path/to/foo.rs` (the latter is a
//! shortcut for `@file`).  Unknown prefixes are ignored and kept in
//! the surrounding prose.

use std::path::PathBuf;

use super::types::ContextTag;

pub fn parse_context_tags(text: &str) -> Vec<ContextTag> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '@' {
            continue;
        }

        let prev = if i == 0 {
            None
        } else {
            text[..i].chars().next_back()
        };
        if matches!(prev, Some(p) if !p.is_whitespace() && !matches!(p, '(' | '[' | '{' | ',')) {
            continue;
        }

        let start = i + 1;
        let end = text[start..]
            .find(|ch: char| ch.is_whitespace())
            .map(|p| start + p)
            .unwrap_or(text.len());
        let body = &text[start..end];
        if body.is_empty() {
            continue;
        }
        if let Some(tag) = classify(body) {
            out.push(tag);
        }

        while let Some(&(pos, _)) = chars.peek() {
            if pos >= end {
                break;
            }
            chars.next();
        }
    }
    out
}

pub fn strip_context_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    let mut last = 0usize;
    while let Some((i, c)) = chars.next() {
        if c != '@' {
            continue;
        }
        let prev = if i == 0 {
            None
        } else {
            text[..i].chars().next_back()
        };
        if matches!(prev, Some(p) if !p.is_whitespace() && !matches!(p, '(' | '[' | '{' | ',')) {
            continue;
        }
        let start = i + 1;
        let end = text[start..]
            .find(|ch: char| ch.is_whitespace())
            .map(|p| start + p)
            .unwrap_or(text.len());
        let body = &text[start..end];
        if classify(body).is_some() {
            out.push_str(&text[last..i]);
            last = end;
            while let Some(&(pos, _)) = chars.peek() {
                if pos >= end {
                    break;
                }
                chars.next();
            }
        }
    }
    out.push_str(&text[last..]);
    out
}

fn classify(body: &str) -> Option<ContextTag> {
    let (prefix, value) = match body.split_once(':') {
        Some((p, v)) => (p.to_ascii_lowercase(), v.to_string()),
        None => {

            match body.to_ascii_lowercase().as_str() {
                "recent" => return Some(ContextTag::Recent),
                "selection" => return Some(ContextTag::Selection),
                _ => {
                    if looks_like_path(body) {
                        return Some(ContextTag::File(PathBuf::from(body)));
                    }
                    return None;
                }
            }
        }
    };
    if value.is_empty() {
        return None;
    }
    Some(match prefix.as_str() {
        "file" => ContextTag::File(PathBuf::from(value)),
        "symbol" => ContextTag::Symbol(value),
        "folder" | "dir" => ContextTag::Folder(PathBuf::from(value)),
        "url" => ContextTag::Url(value),
        "doc" => ContextTag::Doc(value),
        "diff" => ContextTag::Diff(value),
        "test" => ContextTag::Test(value),

        "codebase" => ContextTag::Codebase(value),
        _ => return None,
    })
}

fn looks_like_path(s: &str) -> bool {

    if s.contains('/') || s.contains('\\') {
        return true;
    }

    matches!(
        s.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()),
        Some(ref ext) if matches!(
            ext.as_str(),

            "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "java" |
            "kt" | "kts" | "scala" | "swift" | "c" | "cc" | "cpp" |
            "cxx" | "h" | "hh" | "hpp" | "hxx" | "cs" | "rb" | "php" |
            "pl" | "lua" | "ex" | "exs" | "erl" | "elm" | "dart" |
            "zig" | "v" | "nim" | "f90" | "r" | "jl" | "ml" | "mli" |
            "fs" | "fsx" | "fsi" | "groovy" | "clj" | "cljs" | "edn" |
            "hs" | "lhs" | "sql" | "graphql" | "proto" | "thrift" |

            "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" |
            "toml" | "yaml" | "yml" | "json" | "json5" | "jsonc" |
            "ini" | "cfg" | "conf" | "env" | "lock" | "make" | "cmake" |
            "gradle" | "sbt" | "tf" | "tfvars" | "hcl" | "nix" |

            "md" | "mdx" | "rst" | "adoc" | "txt" | "html" | "htm" |
            "css" | "scss" | "less" | "styl" | "vue" | "svelte" |
            "xml" | "csv" | "tsv"
        )
    )
}
