// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! production [`FileWatcher`] backed by the cross-
//! platform [`notify`] crate.
//!
//! This module is feature-gated behind `fs-watch` so minimal builds
//! (unit tests, `--no-default-features`) continue to drive the
//! incremental SymbolGraph via the [`ManualWatcher`].
//!
//! Wiring:
//!
//! ```text
//! notify::RecommendedWatcher ──► tokio crossbeam channel ──► NotifyWatcher.poll()
//!                                                         ▲
//!                                                         │ non-blocking drain
//!                                              IncrementalBuilder tick loop
//! ```
//!
//! We deduplicate bursts of duplicate `Modify(Data)` / `Create` /
//! `Remove` events at the watcher boundary so callers pay for the
//! *minimum* work per filesystem change.  Final debouncing happens
//! in [`crate::code_intel::symbol_graph_incremental::Debouncer`].

#![cfg(feature = "fs-watch")]

use notify::event::{AccessKind, EventKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use super::symbol_graph_incremental::{FileEvent, FileWatcher};

pub struct NotifyWatcher {
    _watcher: RecommendedWatcher,
    rx: Mutex<Receiver<notify::Result<Event>>>,

    pending: Mutex<VecDeque<FileEvent>>,
}

impl std::fmt::Debug for NotifyWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotifyWatcher").finish_non_exhaustive()
    }
}

impl NotifyWatcher {

    pub fn new(root: &Path) -> notify::Result<Self> {
        let (tx, rx) = channel();
        let config = Config::default().with_poll_interval(Duration::from_millis(500));
        let mut watcher = RecommendedWatcher::new(tx, config)?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            rx: Mutex::new(rx),
            pending: Mutex::new(VecDeque::new()),
        })
    }

    pub fn open(root: &Path) -> std::io::Result<Arc<Self>> {
        Self::new(root)
            .map(Arc::new)
            .map_err(|e| std::io::Error::other(format!("notify: {e}")))
    }
}

impl FileWatcher for NotifyWatcher {
    fn poll(&self) -> Vec<FileEvent> {

        {
            let mut queue = self.pending.lock();
            let rx = self.rx.lock();
            while let Ok(maybe_event) = rx.try_recv() {
                match maybe_event {
                    Ok(ev) => {
                        for translated in translate_event(&ev) {
                            queue.push_back(translated);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(target: "sen::fs_watch", error = %err, "notify backend error");
                    }
                }
            }
        }

        let mut seen: std::collections::HashMap<PathBuf, FileEvent> =
            std::collections::HashMap::new();
        let mut order: Vec<PathBuf> = Vec::new();
        let mut pending = self.pending.lock();
        while let Some(ev) = pending.pop_front() {
            let path = match &ev {
                FileEvent::Changed(p) | FileEvent::Removed(p) => p.clone(),
            };
            if !seen.contains_key(&path) {
                order.push(path.clone());
            }
            seen.insert(path, ev);
        }

        order.into_iter().filter_map(|p| seen.remove(&p)).collect()
    }
}

fn translate_event(ev: &Event) -> Vec<FileEvent> {
    let mut out = Vec::new();
    match ev.kind {
        EventKind::Create(_) => {
            for p in &ev.paths {
                out.push(FileEvent::Changed(p.clone()));
            }
        }
        EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Metadata(_)) => {
            for p in &ev.paths {
                out.push(FileEvent::Changed(p.clone()));
            }
        }
        EventKind::Modify(ModifyKind::Name(mode)) => match mode {
            RenameMode::From => {
                for p in &ev.paths {
                    out.push(FileEvent::Removed(p.clone()));
                }
            }
            RenameMode::To | RenameMode::Any | RenameMode::Other => {
                for p in &ev.paths {
                    out.push(FileEvent::Changed(p.clone()));
                }
            }
            RenameMode::Both => {
                let mut iter = ev.paths.iter();
                if let Some(src) = iter.next() {
                    out.push(FileEvent::Removed(src.clone()));
                }
                if let Some(dst) = iter.next() {
                    out.push(FileEvent::Changed(dst.clone()));
                }
            }
        },
        EventKind::Remove(RemoveKind::File)
        | EventKind::Remove(RemoveKind::Folder)
        | EventKind::Remove(RemoveKind::Any)
        | EventKind::Remove(RemoveKind::Other) => {
            for p in &ev.paths {
                out.push(FileEvent::Removed(p.clone()));
            }
        }
        EventKind::Access(AccessKind::Close(_)) => {
            for p in &ev.paths {
                out.push(FileEvent::Changed(p.clone()));
            }
        }
        _ => {}
    }
    out
}
