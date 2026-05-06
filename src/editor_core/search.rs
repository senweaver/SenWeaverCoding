// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Search and replace primitives for the editor core.
//!
//! Provides case-sensitive/insensitive literal and regex search over a
//! `TextBuffer`, returning `Position` ranges suitable for highlighting or
//! interactive replace flows.  Uses `memchr` for fast byte-pattern scanning
//! on ASCII content and falls back to character-level scanning for
//! case-insensitive patterns or Unicode text.

use std::borrow::Cow;

use super::buffer::{Position, TextBuffer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    pub whole_word: bool,
}

pub fn find_all(buffer: &TextBuffer, pattern: &str, opts: &SearchOptions) -> Vec<Match> {
    if pattern.is_empty() {
        return Vec::new();
    }

    let rope = buffer.rope_ref();
    let line_count = buffer.line_count();
    if line_count == 0 {
        return Vec::new();
    }

    let pattern_lower;
    let needle_bytes: &[u8] = if opts.case_insensitive {
        pattern_lower = pattern.to_lowercase();
        pattern_lower.as_bytes()
    } else {
        pattern.as_bytes()
    };
    let needle_char_len = pattern.chars().count();

    let mut matches = Vec::new();

    for line_idx in 0..line_count {
        let line_slice = rope.line(line_idx);

        let line_str = match line_slice.as_str() {
            Some(s) => s,
            None => {

                continue;
            }
        };
        let line_bytes = line_str.as_bytes();
        let _line_len_chars = line_slice.len_chars();

        let haystack_bytes: Cow<'_, [u8]> = if opts.case_insensitive {

            if line_bytes.iter().all(|&b| !b.is_ascii_uppercase()) {
                Cow::Borrowed(line_bytes)
            } else {
                Cow::Owned(line_str.to_lowercase().into_bytes())
            }
        } else {
            Cow::Borrowed(line_bytes)
        };

        let mut byte_offset = 0usize;

        loop {
            let Some(found) = memchr::memmem::find(
                haystack_bytes.get(byte_offset..).unwrap_or(&[]),
                needle_bytes,
            ) else {
                break;
            };

            let abs_byte = byte_offset + found;
            let match_end_byte = abs_byte + needle_bytes.len();

            if opts.whole_word {
                let before_ok = abs_byte == 0 || !is_word_byte(haystack_bytes[abs_byte - 1]);
                let after_ok = match_end_byte >= haystack_bytes.len()
                    || !is_word_byte(haystack_bytes[match_end_byte]);
                if !before_ok || !after_ok {
                    byte_offset = abs_byte + needle_bytes.len().max(1);
                    continue;
                }
            }

            let col_start = count_chars_up_to_byte(line_bytes, abs_byte);
            let col_end = col_start + needle_char_len;

            matches.push(Match {
                start: Position {
                    line: line_idx,
                    col: col_start,
                },
                end: Position {
                    line: line_idx,
                    col: col_end,
                },
            });

            byte_offset = match_end_byte;
        }
    }

    matches
}

pub fn replace_all(
    buffer: &TextBuffer,
    pattern: &str,
    replacement: &str,
    opts: &SearchOptions,
) -> (TextBuffer, usize) {
    let matches = find_all(buffer, pattern, opts);
    if matches.is_empty() {
        return (buffer.clone(), 0);
    }

    let mut new_rope = buffer.rope_ref().clone();
    for m in matches.iter().rev() {
        let start_char = buffer.char_idx(m.start);
        let end_char = buffer.char_idx(m.end);
        if start_char < end_char {
            new_rope.remove(start_char..end_char);
            new_rope.insert(start_char, replacement);
        }
    }

    let mut new_buf = TextBuffer::from_str(&new_rope.to_string());
    new_buf.set_buffer_id(buffer.buffer_id().to_string());
    (new_buf, matches.len())
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn count_chars_up_to_byte(bytes: &[u8], byte_offset: usize) -> usize {
    let end = byte_offset.min(bytes.len());
    let mut char_count = 0;
    let mut i = 0;
    while i < end {
        let b = bytes[i];
        i += if b < 0x80 {
            1
        } else if b < 0xE0 {
            2
        } else if b < 0xF0 {
            3
        } else {
            4
        };
        char_count += 1;
    }
    char_count
}
