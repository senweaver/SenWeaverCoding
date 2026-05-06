// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Headless code-intelligence primitives.
//!
//! tree-sitter used to live under `gui::treesitter` and
//! was gated behind the `gui` feature, which meant the CLI, TUI, and
//! gateway couldn't run syntax-aware tools.  This module hosts the
//! **shared** helpers (outline extraction, symbol tagging) that any
//! surface can call once the `code-intel` feature is enabled.
//!
//! The heavy tree-sitter logic is in [`outline`].  When the feature
//! is disabled the module still builds and exposes a lightweight
//! heuristic fallback so `tools::code_outline` works on every target
//! — at reduced accuracy, but never as a runtime panic.

#[cfg(feature = "fs-watch")]
pub mod file_watcher_notify;
pub mod git_timeline;

pub mod grammars;
pub mod outline;
pub mod search;
pub mod symbol_graph;

pub mod symbol_graph_incremental;
#[cfg(feature = "fs-watch")]
pub use file_watcher_notify::NotifyWatcher;

pub use git_timeline::{TimelineEntry, build_timeline};
pub use outline::{OutlineEntry, OutlineError, extract_outline, locate_named_scope};
pub use search::{IncrementalIndex, SearchHit};
pub use symbol_graph::{Edge, EdgeKind, SymbolEntry, SymbolGraph, SymbolId};
pub use symbol_graph_incremental::{
    Debouncer, DirtySet, FileEvent, FileWatcher, ManualWatcher, PersistLimiter, pump_events,
};
