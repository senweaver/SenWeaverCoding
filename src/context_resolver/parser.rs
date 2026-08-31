// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::ops::Range;
use std::path::PathBuf;

use super::types::ContextTag;

fn is_tag_terminator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | ';'
                | '!'
                | '?'
                | ')'
                | ']'
                | '}'
                | '<'
                | '>'
                | '"'
                | '\''
                | '`'
                | '，'
                | '。'
                | '、'
                | '；'
                | '：'
                | '！'
                | '？'
                | '（'
                | '）'
                | '【'
                | '】'
                | '《'
                | '》'
                | '「'
                | '」'
                | '『'
                | '』'
                | '\u{201c}'
                | '\u{201d}'
                | '\u{2018}'
                | '\u{2019}'
                | '…'
                | '～'
        )
}

const TRAILING_TAG_PUNCT: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"', '`'];

fn tag_body_span(text: &str, start: usize) -> Range<usize> {
    let scan_end = text[start..]
        .find(is_tag_terminator)
        .map(|p| start + p)
        .unwrap_or(text.len());
    let body = &text[start..scan_end];
    let trimmed = body.trim_end_matches(TRAILING_TAG_PUNCT);
    start..start + trimmed.len()
}

pub fn parse_context_tags_with_spans(text: &str) -> Vec<(ContextTag, Range<usize>)> {
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
        let span = tag_body_span(text, start);
        let body = &text[span.clone()];
        if body.is_empty() {
            continue;
        }
        if let Some(tag) = classify(body) {
            out.push((tag, i..span.end));
        }

        while let Some(&(pos, _)) = chars.peek() {
            if pos >= span.end {
                break;
            }
            chars.next();
        }
    }
    out
}

pub fn parse_context_tags(text: &str) -> Vec<ContextTag> {
    parse_context_tags_with_spans(text)
        .into_iter()
        .map(|(tag, _)| tag)
        .collect()
}

pub fn strip_spans(text: &str, spans: &[Range<usize>]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for span in spans {
        if span.start < last || span.end > text.len() {
            continue;
        }
        out.push_str(&text[last..span.start]);
        last = span.end;
    }
    out.push_str(&text[last..]);
    out
}

pub fn strip_context_tags(text: &str) -> String {
    let spans: Vec<Range<usize>> = parse_context_tags_with_spans(text)
        .into_iter()
        .map(|(_, span)| span)
        .collect();
    strip_spans(text, &spans)
}

fn classify(body: &str) -> Option<ContextTag> {
    let (prefix, value) = match body.split_once(':') {
        Some((p, v)) => (p.to_ascii_lowercase(), v.to_string()),
        None => {

            match body.to_ascii_lowercase().as_str() {
                "recent" => return Some(ContextTag::Recent),
                "selection" => return Some(ContextTag::Selection),
                "problems" | "diagnostics" => return Some(ContextTag::Problems),
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
