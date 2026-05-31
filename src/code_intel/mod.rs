// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(feature = "fs-watch")]
pub mod file_watcher_notify;
pub mod git_timeline;

pub mod grammars;
pub mod outline;
pub mod review;
pub mod search;
pub mod symbol_graph;

#[cfg(feature = "fs-watch")]
pub use file_watcher_notify::NotifyWatcher;

pub use git_timeline::{TimelineEntry, build_timeline};
pub use outline::{OutlineEntry, OutlineError, extract_outline, locate_named_scope};
pub use search::{IncrementalIndex, SearchHit};
pub use symbol_graph::{Edge, EdgeKind, SymbolEntry, SymbolGraph, SymbolId};
pub use symbol_graph::incremental::{
    Debouncer, DirtySet, FileEvent, FileWatcher, ManualWatcher, PersistLimiter, pump_events,
};
