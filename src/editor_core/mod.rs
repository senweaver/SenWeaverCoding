// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
