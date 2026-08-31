// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::security::SecurityPolicy;
use std::path::{Path, PathBuf};

pub fn secure_resolve_target(security: &SecurityPolicy, file: &Path) -> Result<PathBuf, String> {
    if let Ok(meta) = std::fs::symlink_metadata(file) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Refusing to write through symlink: {}",
                file.display()
            ));
        }
    }
    let resolved = std::fs::canonicalize(file)
        .map_err(|e| format!("Failed to resolve {}: {e}", file.display()))?;
    if !security.is_resolved_path_allowed(&resolved) {
        return Err(security.resolved_path_violation_message(&resolved));
    }
    Ok(resolved)
}

pub fn uri_to_local_path(uri: &str) -> PathBuf {
    let stripped = uri
        .trim_start_matches("file:///")
        .trim_start_matches("file://");
    let decoded = percent_decode_uri(stripped);
    if cfg!(windows) {
        PathBuf::from(decoded.replace('/', std::path::MAIN_SEPARATOR_STR))
    } else {
        PathBuf::from(format!("/{decoded}"))
    }
}

fn percent_decode_uri(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn lsp_position_to_byte_offset(content: &str, line: usize, character_utf16: usize) -> usize {
    let bytes = content.as_bytes();
    let mut idx = 0usize;
    let mut current_line = 0usize;
    while current_line < line && idx < bytes.len() {
        if bytes[idx] == b'\n' {
            current_line += 1;
        }
        idx += 1;
    }
    if current_line < line {
        return content.len();
    }
    let mut utf16_count = 0usize;
    let remaining = &content[idx..];
    for ch in remaining.chars() {
        if ch == '\n' {
            break;
        }
        if utf16_count >= character_utf16 {
            break;
        }
        utf16_count += ch.len_utf16();
        idx += ch.len_utf8();
    }
    idx
}

pub fn apply_edits_to_content(
    content: &str,
    edits: &[serde_json::Value],
) -> (String, usize, Vec<String>) {
    let mut errors = Vec::new();
    let mut resolved_edits: Vec<(usize, usize, usize, String)> = edits
        .iter()
        .enumerate()
        .filter_map(|(idx, e)| {
            let sl = e.pointer("/range/start/line")?.as_u64()? as usize;
            let sc = e.pointer("/range/start/character")?.as_u64()? as usize;
            let el = e.pointer("/range/end/line")?.as_u64()? as usize;
            let ec = e.pointer("/range/end/character")?.as_u64()? as usize;
            let new_text = e.get("newText")?.as_str()?.to_string();
            let start = lsp_position_to_byte_offset(content, sl, sc);
            let end = lsp_position_to_byte_offset(content, el, ec);
            Some((start, end, idx, new_text))
        })
        .collect();
    resolved_edits.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(b.2.cmp(&a.2))
    });
    let mut out = content.to_string();
    let mut applied = 0usize;
    for (start, end, _, new_text) in resolved_edits {
        if start > end || end > out.len() {
            errors.push("skipped out-of-range LSP edit".to_string());
            continue;
        }
        if !out.is_char_boundary(start) || !out.is_char_boundary(end) {
            errors.push("skipped LSP edit that did not fall on a char boundary".to_string());
            continue;
        }
        out.replace_range(start..end, &new_text);
        applied += 1;
    }
    (out, applied, errors)
}

pub fn adapt_edit_newtext_eols(
    edits: &[serde_json::Value],
    dominant: &str,
) -> Vec<serde_json::Value> {
    edits
        .iter()
        .map(|e| {
            let mut edit = e.clone();
            if let Some(new_text) = edit.get("newText").and_then(|v| v.as_str()) {
                if let Some(flavor) = crate::tools::file::eol::eol_flavor(new_text) {
                    if flavor != dominant {
                        let adapted =
                            crate::tools::file::eol::adapt_text_to_eol(new_text, dominant);
                        if let Some(slot) = edit.get_mut("newText") {
                            *slot = serde_json::Value::String(adapted);
                        }
                    }
                }
            }
            edit
        })
        .collect()
}
