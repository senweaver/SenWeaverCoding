// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEndForward,
    LineStart,
    LineEnd,
    FirstNonBlank,
    BufferStart,
    BufferEnd,
    FindChar(char),
    FindCharBackward(char),
    TillChar(char),
    TillCharBackward(char),
    MatchBracket,
    SearchForward,
    SearchBackward,
}

pub fn resolve_motion(motion: Motion, text: &str, cursor_char: usize, count: u32) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let char_len = chars.len();
    if char_len == 0 {
        return 0;
    }

    let char_pos = cursor_char.min(char_len.saturating_sub(1));

    match motion {
        Motion::Left => char_pos.saturating_sub(count as usize),
        Motion::Right => (char_pos + count as usize).min(char_len.saturating_sub(1)),
        Motion::LineStart => 0,
        Motion::LineEnd => char_len.saturating_sub(1),
        Motion::FirstNonBlank => chars.iter().position(|c| !c.is_whitespace()).unwrap_or(0),
        Motion::WordForward => {
            let mut pos = char_pos;
            for _ in 0..count {

                while pos < char_len && chars[pos].is_alphanumeric() {
                    pos += 1;
                }

                while pos < char_len && !chars[pos].is_alphanumeric() {
                    pos += 1;
                }
            }
            pos.min(char_len.saturating_sub(1))
        }
        Motion::WordBackward => {
            let mut pos = char_pos;
            for _ in 0..count {
                pos = pos.saturating_sub(1);

                while pos > 0 && !chars[pos].is_alphanumeric() {
                    pos -= 1;
                }

                while pos > 0 && chars[pos - 1].is_alphanumeric() {
                    pos -= 1;
                }
            }
            pos
        }
        Motion::WordEndForward => {
            let mut pos = char_pos;
            for _ in 0..count {
                if pos < char_len.saturating_sub(1) {
                    pos += 1;
                }

                while pos < char_len.saturating_sub(1) && !chars[pos].is_alphanumeric() {
                    pos += 1;
                }

                while pos < char_len.saturating_sub(1) && chars[pos + 1].is_alphanumeric() {
                    pos += 1;
                }
            }
            pos.min(char_len.saturating_sub(1))
        }
        Motion::BufferStart => 0,
        Motion::BufferEnd => char_len.saturating_sub(1),
        Motion::FindChar(c) => {
            let mut pos = char_pos;
            for _ in 0..count {
                pos += 1;
                while pos < char_len && chars[pos] != c {
                    pos += 1;
                }
            }
            pos.min(char_len.saturating_sub(1))
        }
        Motion::FindCharBackward(c) => {
            let mut pos = char_pos;
            for _ in 0..count {
                pos = pos.saturating_sub(1);
                while pos > 0 && chars[pos] != c {
                    pos -= 1;
                }
            }
            pos
        }
        _ => char_pos,
    }
}
