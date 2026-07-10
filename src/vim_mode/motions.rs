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

pub(crate) fn line_start_at(chars: &[char], pos: usize) -> usize {
    let mut i = pos.min(chars.len());
    while i > 0 && chars[i - 1] != '\n' {
        i -= 1;
    }
    i
}

pub(crate) fn line_end_at(chars: &[char], pos: usize) -> usize {
    let mut i = pos.min(chars.len());
    while i < chars.len() && chars[i] != '\n' {
        i += 1;
    }
    i
}

fn move_vertical_up(chars: &[char], pos: usize) -> usize {
    let ls = line_start_at(chars, pos);
    if ls == 0 {
        return pos;
    }
    let col = pos - ls;
    let prev_le = ls - 1;
    let prev_ls = line_start_at(chars, prev_le);
    let prev_len = prev_le - prev_ls;
    prev_ls + col.min(prev_len.saturating_sub(1))
}

fn move_vertical_down(chars: &[char], pos: usize) -> usize {
    let le = line_end_at(chars, pos);
    if le >= chars.len() {
        return pos;
    }
    let col = pos - line_start_at(chars, pos);
    let next_ls = le + 1;
    let next_le = line_end_at(chars, next_ls);
    let next_len = next_le - next_ls;
    (next_ls + col.min(next_len.saturating_sub(1))).min(chars.len().saturating_sub(1))
}

fn find_forward_in_line(chars: &[char], pos: usize, target: char, count: u32) -> Option<usize> {
    let le = line_end_at(chars, pos);
    let mut cur = pos;
    for _ in 0..count.max(1) {
        let mut i = cur + 1;
        while i < le && chars[i] != target {
            i += 1;
        }
        if i >= le {
            return None;
        }
        cur = i;
    }
    Some(cur)
}

fn find_backward_in_line(chars: &[char], pos: usize, target: char, count: u32) -> Option<usize> {
    let ls = line_start_at(chars, pos);
    let mut cur = pos;
    for _ in 0..count.max(1) {
        if cur == ls {
            return None;
        }
        let mut i = cur - 1;
        loop {
            if chars[i] == target {
                break;
            }
            if i == ls {
                return None;
            }
            i -= 1;
        }
        cur = i;
    }
    Some(cur)
}

const BRACKET_PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

fn match_bracket(chars: &[char], pos: usize) -> Option<usize> {
    let le = line_end_at(chars, pos);
    let mut start = pos;
    while start < le
        && !BRACKET_PAIRS
            .iter()
            .any(|(o, c)| chars[start] == *o || chars[start] == *c)
    {
        start += 1;
    }
    if start >= le {
        return None;
    }
    let ch = chars[start];
    if let Some(&(open, close)) = BRACKET_PAIRS.iter().find(|(o, _)| *o == ch) {
        let mut depth = 0usize;
        for (i, &c) in chars.iter().enumerate().skip(start) {
            if c == open {
                depth += 1;
            } else if c == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    } else if let Some(&(open, close)) = BRACKET_PAIRS.iter().find(|(_, c)| *c == ch) {
        let mut depth = 0usize;
        let mut i = start;
        loop {
            if chars[i] == close {
                depth += 1;
            } else if chars[i] == open {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            if i == 0 {
                return None;
            }
            i -= 1;
        }
    } else {
        None
    }
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
        Motion::Up => {
            let mut pos = char_pos;
            for _ in 0..count.max(1) {
                pos = move_vertical_up(&chars, pos);
            }
            pos
        }
        Motion::Down => {
            let mut pos = char_pos;
            for _ in 0..count.max(1) {
                pos = move_vertical_down(&chars, pos);
            }
            pos
        }
        Motion::LineStart => line_start_at(&chars, char_pos),
        Motion::LineEnd => {
            let ls = line_start_at(&chars, char_pos);
            line_end_at(&chars, char_pos).saturating_sub(1).max(ls)
        }
        Motion::FirstNonBlank => {
            let ls = line_start_at(&chars, char_pos);
            let le = line_end_at(&chars, char_pos);
            chars[ls..le]
                .iter()
                .position(|c| !c.is_whitespace())
                .map_or(ls, |offset| ls + offset)
        }
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
            find_forward_in_line(&chars, char_pos, c, count).unwrap_or(char_pos)
        }
        Motion::TillChar(c) => find_forward_in_line(&chars, char_pos, c, count)
            .map_or(char_pos, |target| target.saturating_sub(1).max(char_pos)),
        Motion::FindCharBackward(c) => {
            find_backward_in_line(&chars, char_pos, c, count).unwrap_or(char_pos)
        }
        Motion::TillCharBackward(c) => find_backward_in_line(&chars, char_pos, c, count)
            .map_or(char_pos, |target| (target + 1).min(char_pos)),
        Motion::MatchBracket => match_bracket(&chars, char_pos).unwrap_or(char_pos),
        Motion::SearchForward | Motion::SearchBackward => char_pos,
    }
}
