// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::buffer::{Position, Selection};

#[derive(Debug, Clone)]
pub enum EditOp {
    Insert {
        pos: Position,
        text: String,
    },
    Delete {
        sel: Selection,
        deleted_text: String,
    },
}

pub struct UndoStack {
    undo: Vec<EditOp>,
    redo: Vec<EditOp>,
    max_depth: usize,
}

impl UndoStack {
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_depth,
        }
    }

    pub fn push(&mut self, op: EditOp) {
        self.redo.clear();
        self.undo.push(op);
        if self.undo.len() > self.max_depth {
            self.undo.remove(0);
        }
    }

    pub fn pop_undo(&mut self) -> Option<EditOp> {
        let op = self.undo.pop()?;
        self.redo.push(op.clone());
        Some(op)
    }

    pub fn pop_redo(&mut self) -> Option<EditOp> {
        let op = self.redo.pop()?;
        self.undo.push(op.clone());
        Some(op)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }
}
