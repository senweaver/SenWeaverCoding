// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EolSpan {
    pub start: usize,
    pub end: usize,
    pub had_crlf: bool,
}

pub fn dominant_eol(content: &str) -> &'static str {
    let crlf = content.matches("\r\n").count();
    let total_lf = content.matches('\n').count();
    let bare_lf = total_lf.saturating_sub(crlf);
    if crlf > bare_lf { "\r\n" } else { "\n" }
}

pub fn adapt_text_to_eol(text: &str, eol: &str) -> String {
    let lf = text.replace("\r\n", "\n");
    if eol == "\r\n" {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

pub fn eol_flavor(text: &str) -> Option<&'static str> {
    if text.contains("\r\n") {
        Some("\r\n")
    } else if text.contains('\n') {
        Some("\n")
    } else {
        None
    }
}

pub fn adapt_replacement_eol(old_string: &str, new_string: &str, file_dominant: &str) -> String {
    match (eol_flavor(old_string), eol_flavor(new_string)) {
        (Some(old_eol), Some(new_eol)) if old_eol != new_eol => {
            adapt_text_to_eol(new_string, old_eol)
        }
        (None, Some(new_eol)) if new_eol != file_dominant => {
            adapt_text_to_eol(new_string, file_dominant)
        }
        _ => new_string.to_string(),
    }
}

pub fn adapt_new_text_for_span(new_string: &str, span_had_crlf: bool) -> String {
    let lf = new_string.replace("\r\n", "\n");
    if span_had_crlf {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

pub fn find_eol_insensitive_spans(content: &str, old_string: &str, cap: usize) -> Vec<EolSpan> {
    if cap == 0 {
        return Vec::new();
    }
    if !content.contains('\r') && !old_string.contains('\r') {
        return Vec::new();
    }
    let old_lf = old_string.replace("\r\n", "\n");
    if old_lf.is_empty() {
        return Vec::new();
    }

    let bytes = content.as_bytes();
    let mut normalized: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut offsets: Vec<usize> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            offsets.push(i);
            normalized.push(b'\n');
            i += 2;
        } else {
            offsets.push(i);
            normalized.push(bytes[i]);
            i += 1;
        }
    }

    let finder = memchr::memmem::Finder::new(old_lf.as_bytes());
    let mut spans: Vec<EolSpan> = Vec::new();
    for first in finder.find_iter(&normalized) {
        let norm_end = first + old_lf.len();
        let orig_start = offsets[first];
        let last_norm_idx = norm_end - 1;
        let last_orig_idx = offsets[last_norm_idx];
        let last_width = if bytes[last_orig_idx] == b'\r'
            && last_orig_idx + 1 < bytes.len()
            && bytes[last_orig_idx + 1] == b'\n'
        {
            2
        } else {
            1
        };
        let orig_end = last_orig_idx + last_width;
        let had_crlf = content[orig_start..orig_end].contains("\r\n");
        spans.push(EolSpan {
            start: orig_start,
            end: orig_end,
            had_crlf,
        });
        if spans.len() >= cap {
            break;
        }
    }
    spans
}

pub fn find_eol_insensitive_unique(content: &str, old_string: &str) -> Option<EolSpan> {
    let spans = find_eol_insensitive_spans(content, old_string, 2);
    if spans.len() == 1 { Some(spans[0]) } else { None }
}

pub fn count_matches_eol_insensitive(content: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let exact = memchr::memmem::Finder::new(needle.as_bytes())
        .find_iter(content.as_bytes())
        .count();
    if exact > 0 {
        return exact;
    }
    find_eol_insensitive_spans(content, needle, usize::MAX).len()
}
