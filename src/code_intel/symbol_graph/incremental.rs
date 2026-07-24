// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    sync_signal: Arc<(Mutex<()>, Condvar)>,
}

struct GlobalWriterEntry {
    writer: Arc<SymbolGraphWriter>,
    drain_scheduled: std::sync::atomic::AtomicBool,
    #[cfg(feature = "fs-watch")]
    watcher_started: std::sync::atomic::AtomicBool,
}

static GLOBAL_WRITERS: std::sync::OnceLock<
    RwLock<std::collections::HashMap<PathBuf, Arc<GlobalWriterEntry>>>,
> = std::sync::OnceLock::new();

fn global_writers()
-> &'static RwLock<std::collections::HashMap<PathBuf, Arc<GlobalWriterEntry>>> {
    GLOBAL_WRITERS.get_or_init(|| RwLock::new(std::collections::HashMap::new()))
}

fn persisted_graph_path(root: &Path) -> PathBuf {
    root.join(".sen").join("symbol_graph.json")
}

fn find_graph_root(path: &Path) -> Option<PathBuf> {
    let mut cursor = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    loop {
        if persisted_graph_path(&cursor).is_file() {
            return Some(cursor);
        }
        if !cursor.pop() {
            return None;
        }
    }
}

fn entry_for_root(root: &Path, build_if_missing: bool) -> Option<Arc<GlobalWriterEntry>> {
    if let Some(entry) = global_writers().read().get(root) {
        return Some(Arc::clone(entry));
    }
    let graph = match SymbolGraph::load(root) {
        Ok(Some(g)) => g,
        Ok(None) if build_if_missing => match SymbolGraph::build(root) {
            Ok(g) => {
                let _ = g.persist(root);
                g
            }
            Err(_) => return None,
        },
        _ => return None,
    };
    let writer = Arc::new(SymbolGraphWriter::new(
        Arc::new(RwLock::new(graph)),
        root.to_path_buf(),
    ));
    let entry = Arc::new(GlobalWriterEntry {
        writer,
        drain_scheduled: std::sync::atomic::AtomicBool::new(false),
        #[cfg(feature = "fs-watch")]
        watcher_started: std::sync::atomic::AtomicBool::new(false),
    });
    let mut guard = global_writers().write();
    let stored = match guard.entry(root.to_path_buf()) {
        std::collections::hash_map::Entry::Occupied(slot) => Arc::clone(slot.get()),
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(Arc::clone(&entry));
            entry
        }
    };
    Some(stored)
}

#[cfg(feature = "fs-watch")]
static LEXICAL_ONLY_WATCHERS: std::sync::OnceLock<
    parking_lot::Mutex<std::collections::HashSet<PathBuf>>,
> = std::sync::OnceLock::new();

#[cfg(feature = "fs-watch")]
fn lexical_only_watchers() -> &'static parking_lot::Mutex<std::collections::HashSet<PathBuf>> {
    LEXICAL_ONLY_WATCHERS.get_or_init(|| parking_lot::Mutex::new(std::collections::HashSet::new()))
}

#[cfg(feature = "fs-watch")]
pub fn ensure_workspace_watcher(root: &Path) {
    use std::sync::atomic::Ordering;
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    match entry_for_root(root, false) {
        Some(entry) => {
            if entry
                .watcher_started
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }
            lexical_only_watchers().lock().remove(root);
            spawn_fs_watcher_for_root(root.to_path_buf(), Some(entry));
        }
        None => {
            {
                let mut guard = lexical_only_watchers().lock();
                if !guard.insert(root.to_path_buf()) {
                    return;
                }
            }
            spawn_fs_watcher_for_root(root.to_path_buf(), None);
        }
    }
}

#[cfg(not(feature = "fs-watch"))]
pub fn ensure_workspace_watcher(_root: &Path) {}

#[cfg(feature = "fs-watch")]
fn spawn_fs_watcher_for_root(root: PathBuf, entry: Option<Arc<GlobalWriterEntry>>) {
    fn release_start_flag(entry: &Option<Arc<GlobalWriterEntry>>, root: &Path) {
        match entry {
            Some(e) => e
                .watcher_started
                .store(false, std::sync::atomic::Ordering::Release),
            None => {
                lexical_only_watchers().lock().remove(root);
            }
        }
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        release_start_flag(&entry, &root);
        return;
    };
    let watcher = match crate::code_intel::file_watcher_notify::NotifyWatcher::open(&root) {
        Ok(w) => w,
        Err(err) => {
            release_start_flag(&entry, &root);
            tracing::debug!(
                target: "code_intel.fs_watch",
                root = %root.display(),
                error = %err,
                "workspace fs watcher unavailable; incremental updates limited to agent edits"
            );
            return;
        }
    };
    handle.spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if entry.is_none() && !lexical_only_watchers().lock().contains(&root) {
                return;
            }
            let poll_watcher = std::sync::Arc::clone(&watcher);
            let events = match tokio::task::spawn_blocking(move || poll_watcher.poll()).await {
                Ok(events) => {
                    crate::agent::loop_::services::note_lexical_watcher_alive(&root);
                    events
                }
                Err(_) => continue,
            };
            if events.is_empty() {
                continue;
            }
            let mut changed: Vec<PathBuf> = Vec::new();
            let mut removed: Vec<PathBuf> = Vec::new();
            for ev in events {
                match ev {
                    FileEvent::Changed(p) if watch_relevant(&p) => changed.push(p),
                    FileEvent::Removed(p) if watch_relevant(&p) => removed.push(p),
                    _ => {}
                }
            }
            if changed.is_empty() && removed.is_empty() {
                continue;
            }
            if let Some(entry) = entry.as_ref() {
                if !changed.is_empty() {
                    entry.writer.on_files_changed(&changed);
                }
                if !removed.is_empty() {
                    entry.writer.on_files_removed(&removed);
                }
                schedule_global_drain(Arc::clone(entry));
            }
            if let Some(svc) = crate::services::try_get_services() {
                for p in &changed {
                    svc.lsp.notify_external_change_if_open(p).await;
                }
            }
            let mut all_touched = changed;
            all_touched.extend(removed);
            crate::agent::loop_::services::note_code_files_changed(&all_touched);
        }
    });
}

#[cfg(feature = "fs-watch")]
fn watch_relevant(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    const SKIP_SEGMENTS: &[&str] = &[
        "/.sen/",
        "/.git/",
        "/target/",
        "/node_modules/",
        "/dist/",
        "/build/",
        "/__pycache__/",
        "/.venv/",
    ];
    if SKIP_SEGMENTS.iter().any(|seg| s.contains(seg)) {
        return false;
    }
    crate::agent::loop_::services::is_seedable_source_file(path)
}

#[must_use]
pub fn get_or_load_writer(root: &Path) -> Option<Arc<SymbolGraphWriter>> {
    entry_for_root(root, false).map(|e| Arc::clone(&e.writer))
}

pub fn get_or_build_writer(root: &Path) -> Option<Arc<SymbolGraphWriter>> {
    entry_for_root(root, true).map(|e| Arc::clone(&e.writer))
}

pub enum WriterAvailability {
    Ready(Arc<SymbolGraphWriter>),
    Building,
    Unavailable,
}

static BACKGROUND_BUILDS: std::sync::OnceLock<Mutex<HashSet<PathBuf>>> =
    std::sync::OnceLock::new();

fn background_builds() -> &'static Mutex<HashSet<PathBuf>> {
    BACKGROUND_BUILDS.get_or_init(|| Mutex::new(HashSet::new()))
}

#[must_use]
pub fn get_writer_nonblocking(root: &Path) -> WriterAvailability {
    if let Some(entry) = global_writers().read().get(root) {
        return WriterAvailability::Ready(Arc::clone(&entry.writer));
    }
    if background_builds().lock().contains(root) {
        return WriterAvailability::Building;
    }
    if persisted_graph_path(root).is_file() {
        return match entry_for_root(root, false) {
            Some(entry) => WriterAvailability::Ready(Arc::clone(&entry.writer)),
            None => WriterAvailability::Unavailable,
        };
    }
    if spawn_background_build(root) {
        WriterAvailability::Building
    } else {
        WriterAvailability::Unavailable
    }
}

fn spawn_background_build(root: &Path) -> bool {
    {
        let mut guard = background_builds().lock();
        if !guard.insert(root.to_path_buf()) {
            return true;
        }
    }
    let root_owned = root.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("symbol-graph-build".to_string())
        .spawn(move || {
            let started = Instant::now();
            let built = entry_for_root(&root_owned, true).is_some();
            background_builds().lock().remove(&root_owned);
            tracing::info!(
                target: "code_intel.symbol_graph",
                root = %root_owned.display(),
                built,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "background symbol graph seed finished"
            );
        })
        .is_ok();
    if !spawned {
        background_builds().lock().remove(root);
    }
    spawned
}

pub fn note_files_changed_global(paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    let mut grouped: std::collections::HashMap<PathBuf, Vec<PathBuf>> =
        std::collections::HashMap::new();
    for p in paths {
        if let Some(root) = find_graph_root(p) {
            grouped.entry(root).or_default().push(p.clone());
        }
    }
    for (root, group) in grouped {
        let Some(entry) = entry_for_root(&root, false) else {
            continue;
        };
        entry.writer.on_files_changed(&group);
        schedule_global_drain(entry);
        #[cfg(feature = "fs-watch")]
        ensure_workspace_watcher(&root);
    }
}

pub fn note_files_removed_global(paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    let mut grouped: std::collections::HashMap<PathBuf, Vec<PathBuf>> =
        std::collections::HashMap::new();
    for p in paths {
        if let Some(root) = find_graph_root(p) {
            grouped.entry(root).or_default().push(p.clone());
        }
    }
    for (root, group) in grouped {
        let Some(entry) = entry_for_root(&root, false) else {
            continue;
        };
        entry.writer.on_files_removed(&group);
        schedule_global_drain(entry);
    }
}

fn schedule_global_drain(entry: Arc<GlobalWriterEntry>) {
    use std::sync::atomic::Ordering;
    if entry.drain_scheduled.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        entry.drain_scheduled.store(false, Ordering::SeqCst);
        return;
    };
    handle.spawn(async move {
        tokio::time::sleep(DEFAULT_DEBOUNCE.saturating_add(Duration::from_millis(150))).await;
        entry.drain_scheduled.store(false, Ordering::SeqCst);
        let writer = Arc::clone(&entry.writer);
        let _ = tokio::task::spawn_blocking(move || {
            writer.flush_pending_blocking(Duration::from_secs(2))
        })
        .await;
    });
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
            sync_signal: Arc::new((Mutex::new(()), Condvar::new())),
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
        self.sync_signal.1.notify_all();
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
        self.sync_signal.1.notify_all();
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

        let deadline = Instant::now() + max_wait;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            {
                let mut guard = self.sync_signal.0.lock();
                let _ = self
                    .sync_signal
                    .1
                    .wait_for(&mut guard, deadline.saturating_duration_since(now));
            }
            if self.dirty.lock().is_empty() {
                return false;
            }
            if self.drain_and_rebuild() {
                return true;
            }
        }
    }

    fn drain_and_rebuild(&self) -> bool {
        let (changed, removed) = {
            let mut guard = self.dirty.lock();
            if guard.is_empty() {
                return false;
            }
            guard.drain()
        };
        let payload = {
            let mut graph = self.graph.write();
            graph.partial_rebuild(&changed, &removed, &self.root);
            if self.persist_limiter.try_acquire() {
                let read = parking_lot::RwLockWriteGuard::downgrade(graph);
                read.serialize_for_persist().ok()
            } else {
                None
            }
        };
        if let Some(body) = payload {
            let _ = SymbolGraph::persist_bytes(&self.root, &body);
        }
        crate::observability::code_intel_metrics::incr_symbol_graph_sync_executed();
        true
    }
}
