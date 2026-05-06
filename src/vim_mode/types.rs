// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Vim mode types — mirrors claude-code-typescript-src`vim/types.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
    Replace,
}

impl std::fmt::Display for VimMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "NORMAL"),
            Self::Insert => write!(f, "INSERT"),
            Self::Visual => write!(f, "VISUAL"),
            Self::VisualLine => write!(f, "V-LINE"),
            Self::Command => write!(f, "COMMAND"),
            Self::Replace => write!(f, "REPLACE"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VimState {
    pub mode: VimMode,
    pub cursor_pos: usize,
    pub selection_start: Option<usize>,
    pub pending_operator: Option<VimOperator>,
    pub count: Option<u32>,
    pub register: char,
    pub last_search: Option<String>,
    pub command_buffer: String,
}

impl Default for VimState {
    fn default() -> Self {
        Self {
            mode: VimMode::Normal,
            cursor_pos: 0,
            selection_start: None,
            pending_operator: None,
            count: None,
            register: '"',
            last_search: None,
            command_buffer: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimOperator {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
}

#[derive(Debug, Clone)]
pub enum VimAction {
    ModeChange(VimMode),
    CursorMove(usize),
    InsertChar(char),
    DeleteRange(usize, usize),
    YankRange(usize, usize),
    PasteAfter,
    PasteBefore,
    Undo,
    Redo,
    Submit,
    Cancel,
    NoOp,
}
