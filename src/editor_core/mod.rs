// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared editor core — UI-agnostic text editing primitives.
//!
//! This module provides the pure-logic foundation that CLI `/edit`, TUI
//! editor, and GUI editor all share:
//!
//! - **TextBuffer**: character-indexed buffer backed by `ropey::Rope` (when
//!   the `gui` feature is enabled) or a plain `String` fallback.
//! - **UndoStack**: bounded undo/redo with coalesced edits.
//! - **Cursor / Selection**: line+column addressing, multi-cursor ready.
//!
//! No rendering, no terminal codes, no egui calls — pure data transforms
//! with 100% unit-testable surface.

pub mod buffer;
pub mod motion;
pub mod search;
pub mod undo;

pub use buffer::{MultiSelection, Position, Selection, TextBuffer};
pub use motion::{
    doc_end, doc_start, line_end, line_first_non_whitespace, next_word_start, prev_word_start,
};
pub use search::{Match, SearchOptions, find_all, replace_all};
pub use undo::UndoStack;
