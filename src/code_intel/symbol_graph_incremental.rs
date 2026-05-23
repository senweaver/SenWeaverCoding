// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::{Mutex, RwLock};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

use crate::code_intel::symbol_graph::SymbolGraph;

pub const MIN_PERSIST_INTERVAL: Duration = Duration::from_secs(5);

pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    Changed(PathBuf),
    Removed(PathBuf),
}

pub trait FileWatcher: Send + Sync {
    fn poll(&self) -> Vec<FileEvent>;
}

#[derive(Debug, Default, Clone)]
pub struct ManualWatcher {
    queue: Arc<Mutex<Vec<FileEvent>>>,
}

impl ManualWatcher {
    pub fn push(&self, ev: FileEvent) {
        self.queue.lock().push(ev);
    }
}

impl FileWatcher for ManualWatcher {
    fn poll(&self) -> Vec<FileEvent> {
        std::mem::take(&mut *self.queue.lock())
    }
}

#[derive(Debug, Default)]
pub struct DirtySet {
    changed: HashSet<PathBuf>,
    removed: HashSet<PathBuf>,
}

impl DirtySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, ev: FileEvent) {
        match ev {
            FileEvent::Changed(p) => {
                self.removed.remove(&p);
                self.changed.insert(p);
            }
            FileEvent::Removed(p) => {
                self.changed.remove(&p);
                self.removed.insert(p);
            }
        }
    }

    pub fn changed(&self) -> &HashSet<PathBuf> {
        &self.changed
    }

    pub fn removed(&self) -> &HashSet<PathBuf> {
        &self.removed
    }

    pub fn drain(&mut self) -> (HashSet<PathBuf>, HashSet<PathBuf>) {
        (
            std::mem::take(&mut self.changed),
            std::mem::take(&mut self.removed),
        )
    }

    pub fn len(&self) -> usize {
        self.changed.len() + self.removed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }
}

#[derive(Debug)]
pub struct Debouncer {
    window: Duration,
    last_event: Mutex<Option<Instant>>,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_event: Mutex::new(None),
        }
    }

    pub fn with_default() -> Self {
        Self::new(DEFAULT_DEBOUNCE)
    }

    pub fn notify(&self) {
        *self.last_event.lock() = Some(Instant::now());
    }

    pub fn should_fire(&self) -> bool {
        let guard = self.last_event.lock();
        match *guard {
            Some(at) => at.elapsed() >= self.window,
            None => false,
        }
    }

    pub fn mark_fired(&self) {
        *self.last_event.lock() = None;
    }
}

#[derive(Debug)]
pub struct PersistLimiter {
    min_interval: Duration,
    last_persist: Mutex<Option<Instant>>,
}

impl PersistLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_persist: Mutex::new(None),
        }
    }

    pub fn with_default() -> Self {
        Self::new(MIN_PERSIST_INTERVAL)
    }

    pub fn try_acquire(&self) -> bool {
        let mut guard = self.last_persist.lock();
        match *guard {
            Some(at) if at.elapsed() < self.min_interval => {
                crate::observability::subsystem_metrics::incr_symbol_graph_persist_skipped();
                false
            }
            _ => {
                *guard = Some(Instant::now());
                crate::observability::subsystem_metrics::incr_symbol_graph_rebuild();
                true
            }
        }
    }
}

pub fn pump_events(
    watcher: &dyn FileWatcher,
    dirty: &mut DirtySet,
    debouncer: &Debouncer,
) -> usize {
    let events = watcher.poll();
    let n = events.len();
    if n == 0 {
        return 0;
    }
    for ev in events {
        dirty.apply(ev);
    }
    debouncer.notify();
    n
}

pub fn filter_by_root<'a>(
    paths: impl IntoIterator<Item = &'a PathBuf>,
    root: &Path,
) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|p| p.starts_with(root))
        .cloned()
        .collect()
}

pub struct SymbolGraphWriter {
    graph: Arc<RwLock<SymbolGraph>>,
    root: PathBuf,
    dirty: Arc<Mutex<DirtySet>>,
    debouncer: Arc<Debouncer>,
    persist_limiter: Arc<PersistLimiter>,
    notify: Arc<Notify>,
}

impl SymbolGraphWriter {

    #[must_use]
    pub fn new(graph: Arc<RwLock<SymbolGraph>>, root: PathBuf) -> Self {
        Self {
            graph,
            root,
            dirty: Arc::new(Mutex::new(DirtySet::new())),
            debouncer: Arc::new(Debouncer::with_default()),
            persist_limiter: Arc::new(PersistLimiter::with_default()),
            notify: Arc::new(Notify::new()),
        }
    }

    #[must_use]
    pub fn graph(&self) -> Arc<RwLock<SymbolGraph>> {
        Arc::clone(&self.graph)
    }

    pub fn on_files_changed(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        {
            let mut guard = self.dirty.lock();
            for p in paths {
                guard.apply(FileEvent::Changed(p.clone()));
            }
        }
        self.debouncer.notify();
        self.notify.notify_waiters();
        crate::observability::code_intel_metrics::incr_symbol_graph_sync_scheduled();
    }

    pub fn on_files_removed(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        {
            let mut guard = self.dirty.lock();
            for p in paths {
                guard.apply(FileEvent::Removed(p.clone()));
            }
        }
        self.debouncer.notify();
        self.notify.notify_waiters();
        crate::observability::code_intel_metrics::incr_symbol_graph_sync_scheduled();
    }

    pub fn drain_if_ready(&self) -> bool {
        let ready = self.debouncer.should_fire();
        if !ready {
            if self.dirty.lock().is_empty() {
                return false;
            }

            crate::observability::code_intel_metrics::incr_symbol_graph_sync_debounced();
            return false;
        }
        self.debouncer.mark_fired();
        self.drain_and_rebuild()
    }

    pub fn flush_pending_blocking(&self, max_wait: Duration) -> bool {
        if self.dirty.lock().is_empty() {
            return false;
        }

        self.debouncer.mark_fired();
        if self.drain_and_rebuild() {
            return true;
        }

        let start = Instant::now();
        while start.elapsed() < max_wait {
            std::thread::sleep(Duration::from_millis(10));
            if self.dirty.lock().is_empty() {
                return false;
            }
            if self.drain_and_rebuild() {
                return true;
            }
        }
        false
    }

    fn drain_and_rebuild(&self) -> bool {
        let (changed, removed) = {
            let mut guard = self.dirty.lock();
            if guard.is_empty() {
                return false;
            }
            guard.drain()
        };
        let mut graph = self.graph.write();
        graph.partial_rebuild(&changed, &removed, &self.root);
        if self.persist_limiter.try_acquire() {
            let _ = graph.persist(&self.root);
        }
        crate::observability::code_intel_metrics::incr_symbol_graph_sync_executed();
        true
    }
}
