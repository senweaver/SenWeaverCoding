// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::buffer::{Position, TextBuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    Word,
    Whitespace,
    Punctuation,
}

fn classify(c: char) -> CharClass {
    if c.is_alphanumeric() || c == '_' || (!c.is_ascii() && !c.is_whitespace()) {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Whitespace
    } else {
        CharClass::Punctuation
    }
}

#[inline]
fn char_at(slice: ropey::RopeSlice<'_>, col: usize) -> Option<char> {
    slice.chars().nth(col)
}

pub fn next_word_start(buffer: &TextBuffer, pos: Position) -> Position {
    let line_count = buffer.line_count();
    if line_count == 0 {
        return pos;
    }

    let mut line_idx = pos.line.min(line_count - 1);
    let mut col = pos.col;

    while let Some(line_text) = buffer.line_slice(line_idx) {

        let line_len = line_text.len_chars();
        if col >= line_len {
            line_idx = line_idx.saturating_add(1);
            if line_idx >= line_count {
                let last = line_count.saturating_sub(1);
                return Position {
                    line: last,
                    col: buffer.line_slice(last).map(|s| s.len_chars()).unwrap_or(0),
                };
            }
            col = 0;
            continue;
        }

        let Some(start_class) = char_at(line_text, col) else {
            break;
        };
        let start_class = classify(start_class);

        while col < line_len {
            let Some(c) = char_at(line_text, col) else {
                break;
            };
            if classify(c) != start_class {
                break;
            }
            col += 1;
        }

        while col < line_len {
            let Some(c) = char_at(line_text, col) else {
                break;
            };
            if classify(c) != CharClass::Whitespace {
                break;
            }
            col += 1;
        }

        if col < line_len {
            return Position {
                line: line_idx,
                col,
            };
        }

        line_idx += 1;
        if line_idx >= line_count {
            break;
        }
        col = 0;
    }

    let last = line_count.saturating_sub(1);
    Position {
        line: last,
        col: buffer.line_slice(last).map(|s| s.len_chars()).unwrap_or(0),
    }
}

pub fn prev_word_start(buffer: &TextBuffer, pos: Position) -> Position {
    let line_count = buffer.line_count();
    if line_count == 0 {
        return Position { line: 0, col: 0 };
    }

    let mut line_idx = pos.line;
    let mut col = pos.col;

    loop {
        if col == 0 {
            if line_idx == 0 {
                return Position { line: 0, col: 0 };
            }
            line_idx -= 1;
            col = buffer
                .line_slice(line_idx)
                .map(|s| s.len_chars())
                .unwrap_or(0);
            if col == 0 {
                return Position {
                    line: line_idx,
                    col: 0,
                };
            }
            col -= 1;
        }

        let line_text = match buffer.line_slice(line_idx) {
            Some(s) => s,
            None => return pos,
        };

        let line_len = line_text.len_chars();
        col = col.min(line_len.saturating_sub(1));

        while col > 0 {
            let Some(c) = char_at(line_text, col) else {
                break;
            };
            if classify(c) != CharClass::Whitespace {
                break;
            }
            col -= 1;
        }

        if col == 0 {
            let Some(c) = char_at(line_text, 0) else {
                if line_idx == 0 {
                    return Position { line: 0, col: 0 };
                }
                line_idx -= 1;
                col = buffer
                    .line_slice(line_idx)
                    .map(|s| s.len_chars())
                    .unwrap_or(0);
                continue;
            };
            if classify(c) != CharClass::Whitespace {
                return Position {
                    line: line_idx,
                    col: 0,
                };
            }
            if line_idx == 0 {
                return Position { line: 0, col: 0 };
            }
            line_idx -= 1;
            col = buffer
                .line_slice(line_idx)
                .map(|s| s.len_chars())
                .unwrap_or(0);
            continue;
        }

        let Some(end_class_char) = char_at(line_text, col) else {
            break;
        };
        let end_class = classify(end_class_char);

        while col > 0 {
            let Some(prev_char) = char_at(line_text, col - 1) else {
                break;
            };
            if classify(prev_char) != end_class {
                break;
            }
            col -= 1;
        }

        return Position {
            line: line_idx,
            col,
        };
    }

    Position { line: 0, col: 0 }
}

pub fn line_end(buffer: &TextBuffer, pos: Position) -> Position {
    let line_count = buffer.line_count();
    let line_idx = pos.line.min(line_count.saturating_sub(1));
    let col = buffer
        .line_slice(line_idx)
        .map(|s| s.len_chars())
        .unwrap_or(0);
    Position {
        line: line_idx,
        col,
    }
}

pub fn line_first_non_whitespace(buffer: &TextBuffer, pos: Position) -> Position {
    let line_count = buffer.line_count();
    let line_idx = pos.line.min(line_count.saturating_sub(1));

    let col = match buffer.line_slice(line_idx) {
        Some(line_text) => line_text.chars().take_while(|c| c.is_whitespace()).count(),
        None => 0,
    };
    Position {
        line: line_idx,
        col,
    }
}

pub fn doc_start() -> Position {
    Position { line: 0, col: 0 }
}

pub fn doc_end(buffer: &TextBuffer) -> Position {
    let line_count = buffer.line_count();
    if line_count == 0 {
        return Position { line: 0, col: 0 };
    }
    Position {
        line: line_count - 1,
        col: 0,
    }
}
