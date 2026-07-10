// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::motions::{Motion, line_end_at, line_start_at, resolve_motion};
use super::text_objects::{TextObject, resolve_text_object};
use super::types::{VimAction, VimMode, VimOperator, VimState};

pub fn process_key(state: &mut VimState, key: char, modifiers: &[&str]) -> VimAction {
    process_key_with_buffer(state, key, modifiers, "")
}

pub fn process_key_with_buffer(
    state: &mut VimState,
    key: char,
    modifiers: &[&str],
    buffer: &str,
) -> VimAction {
    match state.mode {
        VimMode::Normal => process_normal_key(state, key, buffer),
        VimMode::Insert => process_insert_key(state, key, modifiers),
        VimMode::Visual | VimMode::VisualLine => process_visual_key(state, key, buffer),
        VimMode::Command => process_command_key(state, key),
        VimMode::Replace => process_replace_key(state, key),
    }
}

fn take_count(state: &mut VimState) -> u32 {
    state.count.take().unwrap_or(1)
}

fn key_to_motion(key: char) -> Option<Motion> {
    match key {
        'h' => Some(Motion::Left),
        'l' => Some(Motion::Right),
        'j' => Some(Motion::Down),
        'k' => Some(Motion::Up),
        'w' => Some(Motion::WordForward),
        'b' => Some(Motion::WordBackward),
        'e' => Some(Motion::WordEndForward),
        '^' => Some(Motion::FirstNonBlank),
        'G' => Some(Motion::BufferEnd),
        '$' => Some(Motion::LineEnd),
        '%' => Some(Motion::MatchBracket),
        _ => None,
    }
}

fn find_key_to_motion(find_key: char, target: char) -> Option<Motion> {
    match find_key {
        'f' => Some(Motion::FindChar(target)),
        'F' => Some(Motion::FindCharBackward(target)),
        't' => Some(Motion::TillChar(target)),
        'T' => Some(Motion::TillCharBackward(target)),
        _ => None,
    }
}

fn resolve_pending_find(
    state: &mut VimState,
    find_key: char,
    target: char,
    buffer: &str,
) -> Option<usize> {
    if target == '\x1b' {
        state.count = None;
        return None;
    }
    let motion = find_key_to_motion(find_key, target)?;
    let count = take_count(state);
    Some(resolve_motion(motion, buffer, state.cursor_pos, count))
}

fn process_normal_key(state: &mut VimState, key: char, buffer: &str) -> VimAction {

    if let Some(find_key) = state.pending_find.take() {
        let Some(target_char) = resolve_pending_find(state, find_key, key, buffer) else {
            return VimAction::NoOp;
        };
        state.cursor_pos = target_char;
        return VimAction::CursorMove(target_char);
    }

    if let Some(op) = state.pending_operator {
        return process_operator_pending(state, op, key, buffer);
    }

    match key {

        'i' => {
            state.mode = VimMode::Insert;
            VimAction::ModeChange(VimMode::Insert)
        }
        'a' => {
            state.mode = VimMode::Insert;
            state.cursor_pos = (state.cursor_pos + 1).min(buffer.chars().count());
            VimAction::ModeChange(VimMode::Insert)
        }
        'I' => {
            state.mode = VimMode::Insert;
            state.cursor_pos = 0;
            VimAction::ModeChange(VimMode::Insert)
        }
        'A' => {
            state.mode = VimMode::Insert;
            state.cursor_pos = buffer.chars().count();
            VimAction::ModeChange(VimMode::Insert)
        }
        'v' => {
            state.mode = VimMode::Visual;
            state.selection_start = Some(state.cursor_pos);
            VimAction::ModeChange(VimMode::Visual)
        }
        'V' => {
            state.mode = VimMode::VisualLine;
            state.selection_start = Some(state.cursor_pos);
            VimAction::ModeChange(VimMode::VisualLine)
        }
        'R' => {
            state.mode = VimMode::Replace;
            VimAction::ModeChange(VimMode::Replace)
        }
        ':' => {
            state.mode = VimMode::Command;
            state.command_buffer.clear();
            VimAction::ModeChange(VimMode::Command)
        }

        'f' | 'F' | 't' | 'T' => {
            state.pending_find = Some(key);
            VimAction::NoOp
        }

        'h' | 'j' | 'k' | 'l' | 'w' | 'b' | 'e' | '^' | 'G' | '$' | '%' => {
            if let Some(motion) = key_to_motion(key) {
                let count = take_count(state);
                let target_char = resolve_motion(motion, buffer, state.cursor_pos, count);
                state.cursor_pos = target_char;
                VimAction::CursorMove(target_char)
            } else {
                VimAction::NoOp
            }
        }
        '0' if state.count.is_none() => {
            state.cursor_pos = 0;
            VimAction::CursorMove(0)
        }

        'd' => {
            state.pending_operator = Some(VimOperator::Delete);
            VimAction::NoOp
        }
        'c' => {
            state.pending_operator = Some(VimOperator::Change);
            VimAction::NoOp
        }
        'y' => {
            state.pending_operator = Some(VimOperator::Yank);
            VimAction::NoOp
        }

        'x' => {
            let pos = state.cursor_pos;
            let char_count = buffer.chars().count();
            if pos < char_count {
                VimAction::DeleteRange(pos, pos + 1)
            } else {
                VimAction::NoOp
            }
        }
        'u' => VimAction::Undo,
        '\x12' => VimAction::Redo,
        'p' => VimAction::PasteAfter,
        'P' => VimAction::PasteBefore,

        '0'..='9' => {
            let digit = key.to_digit(10).unwrap_or(0);
            let current = state.count.unwrap_or(0);
            state.count = Some(current * 10 + digit);
            VimAction::NoOp
        }

        '\x1b' => {
            state.pending_operator = None;
            state.pending_find = None;
            state.count = None;
            state.command_buffer.clear();
            VimAction::NoOp
        }

        _ => VimAction::NoOp,
    }
}

fn process_operator_pending(
    state: &mut VimState,
    op: VimOperator,
    key: char,
    buffer: &str,
) -> VimAction {
    if key == '\x1b' {
        state.pending_operator = None;
        state.pending_find = None;
        state.count = None;
        state.command_buffer.clear();
        return VimAction::NoOp;
    }

    if let Some(find_key) = state.pending_find.take() {
        let cursor_char = state.cursor_pos;
        let Some(target_char) = resolve_pending_find(state, find_key, key, buffer) else {
            state.pending_operator = None;
            return VimAction::NoOp;
        };
        state.pending_operator = None;
        if target_char == cursor_char {
            return VimAction::NoOp;
        }
        let (start, end) = if cursor_char < target_char {
            (cursor_char, target_char + 1)
        } else {
            (target_char, cursor_char)
        };
        return match op {
            VimOperator::Delete => {
                state.cursor_pos = start;
                VimAction::DeleteRange(start, end)
            }
            VimOperator::Change => {
                state.mode = VimMode::Insert;
                state.cursor_pos = start;
                VimAction::DeleteRange(start, end)
            }
            VimOperator::Yank => VimAction::YankRange(start, end),
            _ => VimAction::NoOp,
        };
    }

    if matches!(key, 'f' | 'F' | 't' | 'T') {
        state.pending_find = Some(key);
        return VimAction::NoOp;
    }

    let count = take_count(state);
    state.pending_operator = None;

    let doubled = matches!(
        (op, key),
        (VimOperator::Delete, 'd') | (VimOperator::Change, 'c') | (VimOperator::Yank, 'y')
    );

    if doubled {
        let chars: Vec<char> = buffer.chars().collect();
        let start = line_start_at(&chars, state.cursor_pos);
        let mut end = line_end_at(&chars, state.cursor_pos);
        if end < chars.len() {
            end += 1;
        }
        return match op {
            VimOperator::Yank => VimAction::YankRange(start, end),
            VimOperator::Delete => {
                state.cursor_pos = start;
                VimAction::DeleteRange(start, end)
            }
            VimOperator::Change => {
                state.mode = VimMode::Insert;
                state.cursor_pos = start;
                VimAction::DeleteRange(start, end)
            }
            _ => VimAction::NoOp,
        };
    }

    if key == 'i' || key == 'a' {
        state.pending_operator = Some(op);

        state.command_buffer.clear();
        state.command_buffer.push(key);
        return VimAction::NoOp;
    }

    if let Some(motion) = key_to_motion(key) {
        let cursor_char = state.cursor_pos;
        let target_char = resolve_motion(motion, buffer, cursor_char, count);

        let (start, end) = if cursor_char <= target_char {
            (cursor_char, target_char)
        } else {
            (target_char, cursor_char)
        };

        return match op {
            VimOperator::Delete => {
                state.cursor_pos = start;
                VimAction::DeleteRange(start, end)
            }
            VimOperator::Change => {
                state.mode = VimMode::Insert;
                state.cursor_pos = start;
                VimAction::DeleteRange(start, end)
            }
            VimOperator::Yank => VimAction::YankRange(start, end),
            _ => VimAction::NoOp,
        };
    }

    VimAction::NoOp
}

pub fn process_text_object_key(
    state: &mut VimState,
    op: VimOperator,
    inner_or_around: char,
    key: char,
    buffer: &str,
) -> VimAction {
    let text_obj = match (inner_or_around, key) {
        ('i', 'w') => TextObject::InnerWord,
        ('a', 'w') => TextObject::AWord,
        ('i', '(' | ')' | 'b') => TextObject::InnerParen,
        ('a', '(' | ')' | 'b') => TextObject::AParen,
        ('i', '[' | ']') => TextObject::InnerBracket,
        ('a', '[' | ']') => TextObject::ABracket,
        ('i', '{' | '}' | 'B') => TextObject::InnerBrace,
        ('a', '{' | '}' | 'B') => TextObject::ABrace,
        ('i', '"') => TextObject::InnerQuote('"'),
        ('a', '"') => TextObject::AQuote('"'),
        ('i', '\'') => TextObject::InnerQuote('\''),
        ('a', '\'') => TextObject::AQuote('\''),
        ('i', '`') => TextObject::InnerQuote('`'),
        ('a', '`') => TextObject::AQuote('`'),
        ('i', '<' | '>') => TextObject::InnerAngle,
        ('a', '<' | '>') => TextObject::AAngle,
        _ => {
            state.pending_operator = None;
            state.count = None;
            state.command_buffer.clear();
            return VimAction::NoOp;
        }
    };

    let cursor_byte = buffer
        .char_indices()
        .nth(state.cursor_pos)
        .map(|(b, _)| b)
        .unwrap_or(buffer.len());

    state.pending_operator = None;
    state.command_buffer.clear();

    if let Some((start_byte, end_byte)) = resolve_text_object(text_obj, buffer, cursor_byte) {
        let start_char = buffer[..start_byte].chars().count();
        let end_char = buffer[..end_byte].chars().count();

        match op {
            VimOperator::Delete => {
                state.cursor_pos = start_char;
                VimAction::DeleteRange(start_char, end_char)
            }
            VimOperator::Change => {
                state.mode = VimMode::Insert;
                state.cursor_pos = start_char;
                VimAction::DeleteRange(start_char, end_char)
            }
            VimOperator::Yank => VimAction::YankRange(start_char, end_char),
            _ => VimAction::NoOp,
        }
    } else {
        VimAction::NoOp
    }
}

fn process_insert_key(state: &mut VimState, key: char, modifiers: &[&str]) -> VimAction {
    if key == '\x1b' || (key == '[' && modifiers.contains(&"ctrl")) {
        state.mode = VimMode::Normal;
        return VimAction::ModeChange(VimMode::Normal);
    }
    if key == '\x7f' || key == '\x08' {
        return VimAction::Backspace;
    }
    VimAction::InsertChar(key)
}

fn process_visual_key(state: &mut VimState, key: char, buffer: &str) -> VimAction {
    if let Some(find_key) = state.pending_find.take() {
        let Some(target_char) = resolve_pending_find(state, find_key, key, buffer) else {
            return VimAction::NoOp;
        };
        state.cursor_pos = target_char;
        return VimAction::CursorMove(target_char);
    }

    let linewise = state.mode == VimMode::VisualLine;
    let selection_range = |state: &VimState| -> (usize, usize) {
        let anchor = state.selection_start.unwrap_or(state.cursor_pos);
        let cursor = state.cursor_pos;
        if linewise {
            let chars: Vec<char> = buffer.chars().collect();
            let start = line_start_at(&chars, anchor.min(cursor));
            let mut end = line_end_at(&chars, anchor.max(cursor));
            if end < chars.len() {
                end += 1;
            }
            (start, end)
        } else {
            (anchor.min(cursor), anchor.max(cursor) + 1)
        }
    };

    match key {
        '\x1b' => {
            state.mode = VimMode::Normal;
            state.selection_start = None;
            state.pending_operator = None;
            state.pending_find = None;
            state.count = None;
            state.command_buffer.clear();
            VimAction::ModeChange(VimMode::Normal)
        }
        'f' | 'F' | 't' | 'T' => {
            state.pending_find = Some(key);
            VimAction::NoOp
        }
        'h' | 'j' | 'k' | 'l' | 'w' | 'b' | 'e' | '^' | 'G' | '$' | '%' => {
            if let Some(motion) = key_to_motion(key) {
                let target_char = resolve_motion(motion, buffer, state.cursor_pos, 1);
                state.cursor_pos = target_char;
                VimAction::CursorMove(target_char)
            } else {
                VimAction::NoOp
            }
        }
        '0' => {
            let chars: Vec<char> = buffer.chars().collect();
            let target = line_start_at(&chars, state.cursor_pos);
            state.cursor_pos = target;
            VimAction::CursorMove(target)
        }
        'd' | 'x' => {
            let (start, end) = selection_range(state);
            state.mode = VimMode::Normal;
            state.selection_start = None;
            state.cursor_pos = start;
            VimAction::DeleteRange(start, end)
        }
        'y' => {
            let (start, end) = selection_range(state);
            state.mode = VimMode::Normal;
            state.selection_start = None;
            VimAction::YankRange(start, end)
        }
        'c' => {
            let (start, end) = selection_range(state);
            state.mode = VimMode::Insert;
            state.selection_start = None;
            state.cursor_pos = start;
            VimAction::DeleteRange(start, end)
        }
        _ => VimAction::NoOp,
    }
}

fn process_command_key(state: &mut VimState, key: char) -> VimAction {
    match key {
        '\x1b' => {
            state.mode = VimMode::Normal;
            state.command_buffer.clear();
            VimAction::ModeChange(VimMode::Normal)
        }
        '\n' | '\r' => {
            let cmd = state.command_buffer.trim().to_string();
            state.mode = VimMode::Normal;
            state.command_buffer.clear();
            match cmd.as_str() {
                "q" | "q!" => VimAction::Cancel,
                "w" | "wq" | "x" | "send" => VimAction::Submit,
                "" => VimAction::NoOp,
                other => VimAction::Notice(format!(
                    "vim: unknown command ':{other}' (use :w / :wq / :send to submit, :q to cancel)"
                )),
            }
        }
        '\x7f' | '\x08' => {
            if let Some((idx, _)) = state.command_buffer.char_indices().next_back() {
                state.command_buffer.truncate(idx);
            }
            VimAction::NoOp
        }
        c => {
            state.command_buffer.push(c);
            VimAction::NoOp
        }
    }
}

fn process_replace_key(state: &mut VimState, key: char) -> VimAction {
    if key == '\x1b' {
        state.mode = VimMode::Normal;
        return VimAction::ModeChange(VimMode::Normal);
    }
    if key == '\x7f' || key == '\x08' {
        return VimAction::Backspace;
    }
    VimAction::ReplaceChar(key)
}
