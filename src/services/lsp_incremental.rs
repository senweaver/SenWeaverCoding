// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DidChangeSupport {
    None_,
    Full,
    Incremental,
}

impl DidChangeSupport {

    pub fn from_capability(raw: i64) -> Self {
        match raw {
            0 => Self::None_,
            2 => Self::Incremental,
            _ => Self::Full,
        }
    }
}

pub fn build_full_change(file_uri: &str, version: i32, content: &str) -> Value {
    json!({
        "textDocument": {
            "uri": file_uri,
            "version": version,
        },
        "contentChanges": [{
            "text": content,
        }],
    })
}

pub fn build_incremental_change(
    file_uri: &str,
    version: i32,
    old_text: &str,
    new_text: &str,
) -> Value {
    if old_text == new_text {
        return build_full_change(file_uri, version, new_text);
    }
    let Some(patch) = compute_range_patch(old_text, new_text) else {
        return build_full_change(file_uri, version, new_text);
    };

    json!({
        "textDocument": {
            "uri": file_uri,
            "version": version,
        },
        "contentChanges": [{
            "range": {
                "start": {
                    "line": patch.start_line,
                    "character": patch.start_char,
                },
                "end": {
                    "line": patch.end_line,
                    "character": patch.end_char,
                },
            },
            "rangeLength": patch.range_length,
            "text": patch.new_text,
        }],
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct RangePatch {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,

    pub range_length: u32,
    pub new_text: String,
}

pub fn compute_range_patch(old: &str, new: &str) -> Option<RangePatch> {
    if old == new {
        return None;
    }

    let common_prefix = old
        .as_bytes()
        .iter()
        .zip(new.as_bytes().iter())
        .take_while(|(a, b)| a == b)
        .count();

    let prefix_len = align_to_char_boundary(old, common_prefix);

    let max_suffix = (old.len() - prefix_len).min(new.len() - prefix_len);
    let mut suffix_len = 0usize;
    while suffix_len < max_suffix {
        let a = old.as_bytes()[old.len() - 1 - suffix_len];
        let b = new.as_bytes()[new.len() - 1 - suffix_len];
        if a != b {
            break;
        }
        suffix_len += 1;
    }

    let suffix_len = align_suffix_to_char_boundary(old, new, suffix_len, prefix_len);

    let old_mid_end = old.len() - suffix_len;
    let new_mid_end = new.len() - suffix_len;
    let new_text = new[prefix_len..new_mid_end].to_string();

    let (start_line, start_char) = byte_offset_to_lsp_position(old, prefix_len);
    let (end_line, end_char) = byte_offset_to_lsp_position(old, old_mid_end);
    let range_length = utf16_len(&old[prefix_len..old_mid_end]) as u32;

    Some(RangePatch {
        start_line,
        start_char,
        end_line,
        end_char,
        range_length,
        new_text,
    })
}

fn align_to_char_boundary(s: &str, mut idx: usize) -> usize {
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn align_suffix_to_char_boundary(
    old: &str,
    new: &str,
    mut suffix: usize,
    prefix_len: usize,
) -> usize {
    while suffix > 0 {
        let oi = old.len() - suffix;
        let ni = new.len() - suffix;
        if oi >= prefix_len
            && ni >= prefix_len
            && old.is_char_boundary(oi)
            && new.is_char_boundary(ni)
        {
            return suffix;
        }
        suffix -= 1;
    }
    0
}

fn byte_offset_to_lsp_position(source: &str, byte_offset: usize) -> (u32, u32) {
    let mut line: u32 = 0;
    let mut last_line_start: usize = 0;
    for (i, ch) in source.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            last_line_start = i + 1;
        }
    }
    let line_prefix = &source[last_line_start..byte_offset.min(source.len())];
    let col = utf16_len(line_prefix) as u32;
    (line, col)
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}
