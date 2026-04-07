// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Vim mode transitions — mirrors claude-code-typescript-src`vim/transitions.ts`.

use super::types::{VimAction, VimMode, VimState};

/// Process a key input in the current vim state and return an action.
pub fn process_key(state: &mut VimState, key: char, modifiers: &[&str]) -> VimAction {
    match state.mode {
        VimMode::Normal => process_normal_key(state, key),
        VimMode::Insert => process_insert_key(state, key, modifiers),
        VimMode::Visual | VimMode::VisualLine => process_visual_key(state, key),
        VimMode::Command => process_command_key(state, key),
        VimMode::Replace => process_replace_key(state, key),
    }
}

fn process_normal_key(state: &mut VimState, key: char) -> VimAction {
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
            // cursor_pos set to end by caller
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
        'h' => {
            state.cursor_pos = state.cursor_pos.saturating_sub(1);
            VimAction::CursorMove(state.cursor_pos)
        }
        'l' => {
            state.cursor_pos += 1;
            VimAction::CursorMove(state.cursor_pos)
        }
        '0' if state.count.is_none() => {
            state.cursor_pos = 0;
            VimAction::CursorMove(0)
        }
        '$' => VimAction::CursorMove(usize::MAX), // Caller clamps to line end
        'u' => VimAction::Undo,
        'p' => VimAction::PasteAfter,
        'P' => VimAction::PasteBefore,
        '0'..='9' => {
            let digit = key.to_digit(10).unwrap();
            let current = state.count.unwrap_or(0);
            state.count = Some(current * 10 + digit);
            VimAction::NoOp
        }
        _ => VimAction::NoOp,
    }
}

fn process_insert_key(state: &mut VimState, key: char, modifiers: &[&str]) -> VimAction {
    if key == '\x1b' || (key == '[' && modifiers.contains(&"ctrl")) {
        state.mode = VimMode::Normal;
        return VimAction::ModeChange(VimMode::Normal);
    }
    VimAction::InsertChar(key)
}

fn process_visual_key(state: &mut VimState, key: char) -> VimAction {
    match key {
        '\x1b' => {
            state.mode = VimMode::Normal;
            state.selection_start = None;
            VimAction::ModeChange(VimMode::Normal)
        }
        'h' => {
            state.cursor_pos = state.cursor_pos.saturating_sub(1);
            VimAction::CursorMove(state.cursor_pos)
        }
        'l' => {
            state.cursor_pos += 1;
            VimAction::CursorMove(state.cursor_pos)
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
            // Handle :w, :q, :wq, etc.
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
    // Replace current character
    VimAction::InsertChar(key)
}
