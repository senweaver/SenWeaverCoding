// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObject {
    InnerWord,
    AWord,
    InnerParen,
    AParen,
    InnerBracket,
    ABracket,
    InnerBrace,
    ABrace,
    InnerQuote(char),
    AQuote(char),
    InnerAngle,
    AAngle,
}

pub fn resolve_text_object(obj: TextObject, text: &str, cursor: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let char_len = chars.len();
    if char_len == 0 {
        return None;
    }
    let cursor_byte = crate::util::floor_char_boundary(text, cursor.min(text.len()));
    let char_pos = text[..cursor_byte]
        .chars()
        .count()
        .min(char_len.saturating_sub(1));

    match obj {
        TextObject::InnerWord | TextObject::AWord => {
            let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
            let mut start = char_pos;
            let mut end = char_pos;

            if is_word_char(chars[char_pos]) {
                while start > 0 && is_word_char(chars[start - 1]) {
                    start -= 1;
                }
                while end < char_len - 1 && is_word_char(chars[end + 1]) {
                    end += 1;
                }
            }

            if matches!(obj, TextObject::AWord) {

                while end < char_len - 1 && chars[end + 1].is_whitespace() {
                    end += 1;
                }
            }

            let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
            let byte_end: usize = chars[..=end].iter().map(|c| c.len_utf8()).sum();
            Some((byte_start, byte_end))
        }
        TextObject::InnerParen | TextObject::AParen => {
            find_matching_pair(text, cursor, '(', ')', matches!(obj, TextObject::AParen))
        }
        TextObject::InnerBracket | TextObject::ABracket => {
            find_matching_pair(text, cursor, '[', ']', matches!(obj, TextObject::ABracket))
        }
        TextObject::InnerBrace | TextObject::ABrace => {
            find_matching_pair(text, cursor, '{', '}', matches!(obj, TextObject::ABrace))
        }
        TextObject::InnerQuote(q) | TextObject::AQuote(q) => {
            let include_delimiters = matches!(obj, TextObject::AQuote(_));
            find_matching_quotes(text, cursor, q, include_delimiters)
        }
        TextObject::InnerAngle | TextObject::AAngle => {
            find_matching_pair(text, cursor, '<', '>', matches!(obj, TextObject::AAngle))
        }
    }
}

fn find_matching_pair(
    text: &str,
    cursor: usize,
    open: char,
    close: char,
    include_delimiters: bool,
) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let len = text.len();

    let mut depth = 0i32;
    let mut start = None;
    for i in (0..=cursor.min(len.saturating_sub(1))).rev() {
        if bytes[i] == close as u8 && i != cursor {
            depth += 1;
        } else if bytes[i] == open as u8 {
            if depth == 0 {
                start = Some(i);
                break;
            }
            depth -= 1;
        }
    }

    let start = start?;

    depth = 0;
    let mut end = None;
    for (i, b) in bytes.iter().enumerate().skip(start + 1).take(len.saturating_sub(start + 1)) {
        if *b == open as u8 {
            depth += 1;
        } else if *b == close as u8 {
            if depth == 0 {
                end = Some(i);
                break;
            }
            depth -= 1;
        }
    }

    let end = end?;

    if include_delimiters {
        Some((start, end + 1))
    } else {
        Some((start + 1, end))
    }
}

fn find_matching_quotes(
    text: &str,
    cursor: usize,
    quote: char,
    include_delimiters: bool,
) -> Option<(usize, usize)> {
    let q = quote as u8;
    let bytes = text.as_bytes();
    let len = text.len();

    let mut positions = Vec::new();
    let mut i = 0;
    while i < len {
        if bytes[i] == q && (i == 0 || bytes[i - 1] != b'\\') {
            positions.push(i);
        }
        i += 1;
    }

    for pair in positions.windows(2) {
        let (s, e) = (pair[0], pair[1]);
        if cursor >= s && cursor <= e {
            return if include_delimiters {
                Some((s, e + 1))
            } else {
                Some((s + 1, e))
            };
        }
    }

    None
}
