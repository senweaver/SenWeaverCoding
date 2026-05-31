// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::motions::{Motion, resolve_motion};
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
        'w' => Some(Motion::WordForward),
        'b' => Some(Motion::WordBackward),
        'e' => Some(Motion::WordEndForward),
        '^' => Some(Motion::FirstNonBlank),
        'G' => Some(Motion::BufferEnd),
        '$' => Some(Motion::LineEnd),
        _ => None,
    }
}

fn process_normal_key(state: &mut VimState, key: char, buffer: &str) -> VimAction {

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
            state.cursor_pos += 1;
            VimAction::ModeChange(VimMode::Insert)
        }
        'I' => {
            state.mode = VimMode::Insert;
            state.cursor_pos = 0;
            VimAction::ModeChange(VimMode::Insert)
        }
        'A' => {
            state.mode = VimMode::Insert;
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

        'h' | 'l' | 'w' | 'b' | 'e' | '^' | 'G' => {
            if let Some(motion) = key_to_motion(key) {
                let count = take_count(state);
                let target = resolve_motion(motion, buffer, state.cursor_pos, count);
                let char_pos = buffer[..target.min(buffer.len())].chars().count();
                state.cursor_pos = char_pos;
                VimAction::CursorMove(char_pos)
            } else {
                VimAction::NoOp
            }
        }
        '0' if state.count.is_none() => {
            state.cursor_pos = 0;
            VimAction::CursorMove(0)
        }
        '$' => {
            let count = take_count(state);
            let target = resolve_motion(Motion::LineEnd, buffer, state.cursor_pos, count);
            let char_pos = buffer[..target.min(buffer.len())].chars().count();
            state.cursor_pos = char_pos;
            VimAction::CursorMove(char_pos)
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

        _ => VimAction::NoOp,
    }
}

fn process_operator_pending(
    state: &mut VimState,
    op: VimOperator,
    key: char,
    buffer: &str,
) -> VimAction {
    let count = take_count(state);
    state.pending_operator = None;

    let doubled = matches!(
        (op, key),
        (VimOperator::Delete, 'd') | (VimOperator::Change, 'c') | (VimOperator::Yank, 'y')
    );

    if doubled {
        let char_len = buffer.chars().count();
        return match op {
            VimOperator::Yank => VimAction::YankRange(0, char_len),
            VimOperator::Delete => VimAction::DeleteRange(0, char_len),
            VimOperator::Change => {
                state.mode = VimMode::Insert;
                VimAction::DeleteRange(0, char_len)
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
        let cursor_byte = buffer
            .char_indices()
            .nth(state.cursor_pos)
            .map(|(b, _)| b)
            .unwrap_or(buffer.len());
        let target_byte = resolve_motion(motion, buffer, cursor_byte, count);
        let cursor_char = state.cursor_pos;
        let target_char = buffer[..target_byte.min(buffer.len())].chars().count();

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
    VimAction::InsertChar(key)
}

fn process_visual_key(state: &mut VimState, key: char, buffer: &str) -> VimAction {
    match key {
        '\x1b' => {
            state.mode = VimMode::Normal;
            state.selection_start = None;
            VimAction::ModeChange(VimMode::Normal)
        }
        'h' | 'l' | 'w' | 'b' | 'e' => {
            if let Some(motion) = key_to_motion(key) {
                let cursor_byte = buffer
                    .char_indices()
                    .nth(state.cursor_pos)
                    .map(|(b, _)| b)
                    .unwrap_or(buffer.len());
                let target = resolve_motion(motion, buffer, cursor_byte, 1);
                let char_pos = buffer[..target.min(buffer.len())].chars().count();
                state.cursor_pos = char_pos;
                VimAction::CursorMove(char_pos)
            } else {
                VimAction::NoOp
            }
        }
        'd' | 'x' => {
            let start = state.selection_start.unwrap_or(state.cursor_pos);
            let end = state.cursor_pos;
            state.mode = VimMode::Normal;
            state.selection_start = None;
            VimAction::DeleteRange(start.min(end), start.max(end) + 1)
        }
        'y' => {
            let start = state.selection_start.unwrap_or(state.cursor_pos);
            let end = state.cursor_pos;
            state.mode = VimMode::Normal;
            state.selection_start = None;
            VimAction::YankRange(start.min(end), start.max(end) + 1)
        }
        'c' => {
            let start = state.selection_start.unwrap_or(state.cursor_pos);
            let end = state.cursor_pos;
            state.mode = VimMode::Insert;
            state.selection_start = None;
            VimAction::DeleteRange(start.min(end), start.max(end) + 1)
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
            let cmd = state.command_buffer.clone();
            state.mode = VimMode::Normal;
            state.command_buffer.clear();
            match cmd.as_str() {
                "q" => VimAction::Cancel,
                _ => VimAction::Submit,
            }
        }
        '\x7f' | '\x08' => {
            state.command_buffer.pop();
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
    VimAction::InsertChar(key)
}
