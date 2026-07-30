// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use super::sharded::map::ShardedMap;
use crate::observability::coordination_metrics;

fn ensure_session_scoped_key(key: String) -> String {
    if key.contains("::") || key.starts_with("tool_cache:") {
        return key;
    }
    match crate::session::current_session_context() {
        Some(ctx) if !ctx.session_id.is_empty() => format!("{}::{key}", ctx.session_id),
        _ => format!("__global__::{key}"),
    }
}

fn ensure_session_scoped_namespace(namespace: String) -> String {
    match crate::session::current_session_context() {
        Some(ctx) if !ctx.session_id.is_empty() => {
            let suffix = format!(":{}", ctx.session_id);
            if namespace.ends_with(&suffix) {
                namespace
            } else {
                format!("{namespace}{suffix}")
            }
        }
        _ => namespace,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardEntry {

    pub key: String,

    pub value: serde_json::Value,

    pub owner: String,

    pub version: u64,

    pub created_at: DateTime<Utc>,

    pub updated_at: DateTime<Utc>,

    pub namespace: String,

    #[serde(skip)]
    pub ttl: Option<Duration>,

    #[serde(skip)]
    pub ttl_start: Option<Instant>,
}

impl BlackboardEntry {

    pub fn is_expired(&self) -> bool {
        if let (Some(ttl), Some(start)) = (self.ttl, self.ttl_start) {
            start.elapsed() >= ttl
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardChange {

    pub key: String,

    pub namespace: String,

    pub kind: ChangeKind,

    pub agent: String,

    pub version: u64,

    #[serde(default)]
    pub seq: u64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {

    Created,

    Updated,

    Deleted,
}

#[derive(Debug, thiserror::Error, Clone)]
pub enum BlackboardError {
    #[error("Version conflict on key '{key}': expected {expected}, found {actual}")]
    VersionConflict {
        key: String,
        expected: u64,
        actual: u64,
    },
    #[error("Key '{0}' not found")]
    NotFound(String),
}

const CHANGE_CHANNEL_CAPACITY: usize = 4096;
const JOURNAL_WRITE_QUEUE_CAPACITY: usize = 8192;
const TOMBSTONE_TTL: Duration = Duration::from_secs(3600);
const MAX_IN_MEMORY_CHANGES: usize = 4096;
const JOURNAL_ROTATE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
struct Tombstone {
    version: u64,
    deleted_at: Instant,
}

impl Tombstone {
    fn is_expired(&self) -> bool {
        self.deleted_at.elapsed() >= TOMBSTONE_TTL
    }
}

pub struct ReplaySlice {
    pub changes: Vec<BlackboardChange>,
    pub complete: bool,
}

struct JournalBuffer {
    changes: Vec<BlackboardChange>,
    evicted_through: u64,
}

pub struct BlackboardJournal {
    writer: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    path: PathBuf,
    in_memory: parking_lot::Mutex<JournalBuffer>,
}

impl BlackboardJournal {
    pub fn open(dir: &Path, session: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{session}.jsonl"));

        let mut kept: std::collections::VecDeque<BlackboardChange> =
            std::collections::VecDeque::with_capacity(MAX_IN_MEMORY_CHANGES.min(1024));
        let mut evicted_through = 0u64;
        if let Ok(file) = std::fs::File::open(&path) {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(file);
            for line in reader.lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(rec) = serde_json::from_str::<BlackboardChange>(&line) {
                    if kept.len() >= MAX_IN_MEMORY_CHANGES {
                        if let Some(dropped) = kept.pop_front() {
                            evicted_through = evicted_through.max(dropped.seq);
                        }
                    }
                    kept.push_back(rec);
                }
            }
        }

        let lock_path = dir.join(format!("{session}.jsonl.lock"));
        let lock = match crate::session::write_lock::SessionWriteLock::acquire(&lock_path) {
            Ok(lock) => lock,
            Err(e) => {
                warn!(
                    lock = %lock_path.display(),
                    "failed to probe blackboard journal lock: {e}; persistence disabled"
                );
                None
            }
        };

        let writer = match lock {
            Some(lock) => {
                let oversized = std::fs::metadata(&path)
                    .map(|m| m.len() > JOURNAL_ROTATE_BYTES)
                    .unwrap_or(false);
                if oversized {
                    let mut tail = String::new();
                    for rec in &kept {
                        if let Ok(line) = serde_json::to_string(rec) {
                            tail.push_str(&line);
                            tail.push('\n');
                        }
                    }
                    if let Err(e) = std::fs::write(&path, tail) {
                        warn!(
                            path = %path.display(),
                            "blackboard journal open-time compaction failed: {e}"
                        );
                    }
                }
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)?;
                let (writer, rx) =
                    std::sync::mpsc::sync_channel::<Vec<u8>>(JOURNAL_WRITE_QUEUE_CAPACITY);
                let thread_path = path.clone();
                let spawned = std::thread::Builder::new()
                    .name("blackboard-journal".to_string())
                    .spawn(move || {
                        use std::io::Write;
                        let mut file = file;
                        let mut approx_size = file.metadata().map(|m| m.len()).unwrap_or(0);
                        let mut last_touch = Instant::now();
                        loop {
                            match rx.recv_timeout(Duration::from_secs(30)) {
                                Ok(bytes) => {
                                    if let Err(e) = file.write_all(&bytes) {
                                        warn!("blackboard journal append failed: {e}");
                                    } else {
                                        approx_size =
                                            approx_size.saturating_add(bytes.len() as u64);
                                    }
                                    if approx_size > JOURNAL_ROTATE_BYTES {
                                        match compact_journal_file(&thread_path, &mut file) {
                                            Ok(len) => approx_size = len,
                                            Err(e) => {
                                                warn!(
                                                    path = %thread_path.display(),
                                                    "blackboard journal compaction failed: {e}"
                                                );
                                                approx_size = 0;
                                            }
                                        }
                                    }
                                    if last_touch.elapsed() >= Duration::from_secs(5) {
                                        lock.touch();
                                        last_touch = Instant::now();
                                    }
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                    lock.touch();
                                    last_touch = Instant::now();
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        drop(lock);
                    });
                match spawned {
                    Ok(_) => Some(writer),
                    Err(e) => {
                        warn!(
                            "blackboard journal writer thread failed to start: {e}; persistence disabled"
                        );
                        None
                    }
                }
            }
            None => {
                warn!(
                    path = %path.display(),
                    "another process is journaling this blackboard session; persistence disabled (in-memory only)"
                );
                None
            }
        };

        Ok(Self {
            writer,
            path,
            in_memory: parking_lot::Mutex::new(JournalBuffer {
                changes: kept.into_iter().collect(),
                evicted_through,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn persists(&self) -> bool {
        self.writer.is_some()
    }

    pub fn append(&self, change: &BlackboardChange) {
        if let Some(writer) = self.writer.as_ref() {
            if let Ok(mut line) = serde_json::to_string(change) {
                line.push('\n');
                match writer.try_send(line.into_bytes()) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {
                        coordination_metrics::incr_blackboard_journal_dropped();
                        warn!(
                            path = %self.path.display(),
                            seq = change.seq,
                            "blackboard journal write queue full; dropping journal record"
                        );
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        coordination_metrics::incr_blackboard_journal_dropped();
                    }
                }
            }
        }
        let mut buf = self.in_memory.lock();
        buf.changes.push(change.clone());
        if buf.changes.len() > MAX_IN_MEMORY_CHANGES {
            let drop_count = buf.changes.len() - MAX_IN_MEMORY_CHANGES;
            let dropped_max = buf
                .changes
                .iter()
                .take(drop_count)
                .map(|c| c.seq)
                .max()
                .unwrap_or(0);
            buf.evicted_through = buf.evicted_through.max(dropped_max);
            buf.changes.drain(0..drop_count);
        }
    }

    pub fn replay_since(&self, since: u64) -> ReplaySlice {
        let buf = self.in_memory.lock();
        let mut changes: Vec<BlackboardChange> = buf
            .changes
            .iter()
            .filter(|c| c.seq > since)
            .cloned()
            .collect();
        let complete = since >= buf.evicted_through;
        drop(buf);
        changes.sort_by_key(|c| c.seq);
        ReplaySlice { changes, complete }
    }

    pub fn read_file_since(&self, since: u64) -> std::io::Result<Vec<BlackboardChange>> {
        use std::io::BufRead;
        let file = std::fs::File::open(&self.path)?;
        let reader = std::io::BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(rec) = serde_json::from_str::<BlackboardChange>(&line) {
                if rec.seq > since {
                    out.push(rec);
                }
            }
        }
        out.sort_by_key(|c| c.seq);
        Ok(out)
    }
}

fn compact_journal_file(path: &Path, file: &mut std::fs::File) -> std::io::Result<u64> {
    use std::io::Write;
    let content = std::fs::read_to_string(path)?;
    let max_bytes = (JOURNAL_ROTATE_BYTES / 2) as usize;
    let mut kept: Vec<&str> = Vec::new();
    let mut bytes = 0usize;
    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let add = line.len() + 1;
        if kept.len() >= MAX_IN_MEMORY_CHANGES || bytes + add > max_bytes {
            break;
        }
        kept.push(line);
        bytes += add;
    }
    let mut tail = String::with_capacity(bytes);
    for line in kept.iter().rev() {
        tail.push_str(line);
        tail.push('\n');
    }
    file.set_len(0)?;
    file.write_all(tail.as_bytes())?;
    file.flush()?;
    Ok(tail.len() as u64)
}

pub struct Blackboard {
    entries: ShardedMap<BlackboardEntry>,
    tombstones: ShardedMap<Tombstone>,
    change_sender: broadcast::Sender<BlackboardChange>,
    seq: AtomicU64,
    journal: Option<Arc<BlackboardJournal>>,
    write_count: AtomicU64,
}

const EVICT_EVERY_N_WRITES: u64 = 256;

impl Blackboard {

    pub fn new() -> Self {
        let (change_sender, _rx) = broadcast::channel(CHANGE_CHANNEL_CAPACITY);
        Self {
            entries: ShardedMap::new(),
            tombstones: ShardedMap::new(),
            change_sender,
            seq: AtomicU64::new(0),
            journal: None,
            write_count: AtomicU64::new(0),
        }
    }

    pub fn with_persistence(journal_dir: Option<PathBuf>, session: impl AsRef<str>) -> Self {
        let mut bb = Self::new();
        if let Some(dir) = journal_dir {
            match BlackboardJournal::open(&dir, session.as_ref()) {
                Ok(journal) => {

                    let (records, evicted_through) = {
                        let buf = journal.in_memory.lock();
                        (buf.changes.clone(), buf.evicted_through)
                    };
                    let last_seq = records
                        .iter()
                        .map(|c| c.seq)
                        .max()
                        .unwrap_or(0)
                        .max(evicted_through);
                    bb.hydrate_from_changes(&records);
                    bb.seq = AtomicU64::new(last_seq);
                    bb.journal = Some(Arc::new(journal));
                }
                Err(e) => {
                    warn!(
                        dir = %dir.display(),
                        session = %session.as_ref(),
                        "blackboard journal open failed: {e}; persistence disabled"
                    );
                }
            }
        }
        bb
    }

    fn hydrate_from_changes(&self, changes: &[BlackboardChange]) {
        let now = Utc::now();
        let mut restored = 0usize;
        for change in changes {
            match change.kind {
                ChangeKind::Created | ChangeKind::Updated => {
                    let Some(value) = change.value.clone() else {
                        continue;
                    };
                    let ttl = change.ttl_ms.map(Duration::from_millis);
                    let key = change.key.clone();
                    let namespace = change.namespace.clone();
                    let agent = change.agent.clone();
                    let version = change.version;
                    let applied = self.entries.with_shard_mut(&key, |shard| {
                        if shard.get(&key).is_some_and(|e| e.version >= version) {
                            return false;
                        }
                        let entry = BlackboardEntry {
                            key: key.clone(),
                            value: value.clone(),
                            owner: agent.clone(),
                            version,
                            created_at: now,
                            updated_at: now,
                            namespace: namespace.clone(),
                            ttl,
                            ttl_start: ttl.map(|_| Instant::now()),
                        };
                        shard.insert(key.clone(), entry);
                        true
                    });
                    if applied {
                        self.tombstones.compute(&key, |shard| {
                            if shard.get(&key).is_some_and(|t| t.version <= version) {
                                shard.remove(&key);
                            }
                        });
                        restored += 1;
                    }
                }
                ChangeKind::Deleted => {
                    let key = change.key.clone();
                    let version = change.version;
                    let removed = self.entries.with_shard_mut(&key, |shard| {
                        if shard.get(&key).is_some_and(|e| e.version >= version) {
                            return false;
                        }
                        shard.remove(&key);
                        true
                    });
                    if removed {
                        self.tombstones.compute(&key, |shard| {
                            let stale = shard.get(&key).is_some_and(|t| t.version >= version);
                            if !stale {
                                shard.insert(
                                    key.clone(),
                                    Tombstone {
                                        version,
                                        deleted_at: Instant::now(),
                                    },
                                );
                            }
                        });
                    }
                }
            }
        }
        if restored > 0 {
            debug!(restored, "hydrated blackboard entries from journal");
        }
    }

    pub fn shard_count(&self) -> usize {
        self.entries.shard_count()
    }

    pub fn write(
        &self,
        key: impl Into<String>,
        value: serde_json::Value,
        agent: impl Into<String>,
        namespace: impl Into<String>,
    ) -> u64 {
        self.write_inner(key, value, agent, namespace, None)
    }

    pub fn write_with_ttl(
        &self,
        key: impl Into<String>,
        value: serde_json::Value,
        agent: impl Into<String>,
        namespace: impl Into<String>,
        ttl: Duration,
    ) -> u64 {
        self.write_inner(key, value, agent, namespace, Some(ttl))
    }

    fn write_inner(
        &self,
        key: impl Into<String>,
        value: serde_json::Value,
        agent: impl Into<String>,
        namespace: impl Into<String>,
        ttl: Option<Duration>,
    ) -> u64 {
        let key = ensure_session_scoped_key(key.into());
        let agent = agent.into();
        let namespace = ensure_session_scoped_namespace(namespace.into());
        let now = Utc::now();
        let value_for_journal = value.clone();

        let (version, kind, seq) = self.entries.with_shard_mut(&key, |shard| {
            if let Some(existing) = shard.get_mut(&key) {
                existing.value = value;
                existing.owner = agent.clone();
                existing.version += 1;
                existing.updated_at = now;
                if let Some(ttl) = ttl {
                    existing.ttl = Some(ttl);
                    existing.ttl_start = Some(Instant::now());
                } else {
                    existing.ttl_start = existing.ttl.map(|_| Instant::now());
                }
                let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
                (existing.version, ChangeKind::Updated, seq)
            } else {
                let version = self.take_tombstone_version(&key) + 1;
                let entry = BlackboardEntry {
                    key: key.clone(),
                    value,
                    owner: agent.clone(),
                    version,
                    created_at: now,
                    updated_at: now,
                    namespace: namespace.clone(),
                    ttl,
                    ttl_start: ttl.map(|_| Instant::now()),
                };
                shard.insert(key.clone(), entry);
                let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
                (version, ChangeKind::Created, seq)
            }
        });

        let change = BlackboardChange {
            key: key.clone(),
            namespace,
            kind,
            agent,
            version,
            seq,
            value: None,
            ttl_ms: None,
        };
        self.publish_change(change, Some(value_for_journal), ttl);
        debug!(key = %key, version, "blackboard write");
        version
    }

    fn take_tombstone_version(&self, key: &str) -> u64 {
        match self.tombstones.remove(key) {
            Some(t) if !t.is_expired() => t.version,
            _ => 0,
        }
    }

    fn tombstone_version(&self, key: &str) -> u64 {
        self.tombstones
            .get_cloned(key)
            .filter(|t| !t.is_expired())
            .map(|t| t.version)
            .unwrap_or(0)
    }

    pub fn tool_cache_key(session_id: &str, tool_name: &str, fingerprint: &str) -> String {
        format!("tool_cache:{session_id}:{tool_name}:{fingerprint}")
    }

    pub fn put_tool_result(
        &self,
        session_id: &str,
        tool_name: &str,
        fingerprint: &str,
        value: serde_json::Value,
        ttl: Duration,
    ) -> u64 {
        let key = Self::tool_cache_key(session_id, tool_name, fingerprint);
        self.write_with_ttl(key, value, tool_name, "tool_cache", ttl)
    }

    pub fn get_fresh_tool_result(
        &self,
        session_id: &str,
        tool_name: &str,
        fingerprint: &str,
    ) -> Option<serde_json::Value> {
        let key = Self::tool_cache_key(session_id, tool_name, fingerprint);
        self.get_value(&key)
    }

    pub fn compare_and_swap(
        &self,
        key: impl Into<String>,
        value: serde_json::Value,
        agent: impl Into<String>,
        namespace: impl Into<String>,
        expected_version: u64,
    ) -> Result<u64, BlackboardError> {
        let key = ensure_session_scoped_key(key.into());
        let agent = agent.into();
        let namespace = ensure_session_scoped_namespace(namespace.into());
        let now = Utc::now();
        let value_for_journal = value.clone();

        let cas_result: Result<(u64, ChangeKind, u64), BlackboardError> =
            self.entries.with_shard_mut(&key, |shard| {
                let current_version = shard
                    .get(&key)
                    .map(|e| e.version)
                    .unwrap_or_else(|| self.tombstone_version(&key));
                if current_version != expected_version {
                    return Err(BlackboardError::VersionConflict {
                        key: key.clone(),
                        expected: expected_version,
                        actual: current_version,
                    });
                }

                let (version, kind) = if let Some(existing) = shard.get_mut(&key) {
                    existing.value = value;
                    existing.owner = agent.clone();
                    existing.version += 1;
                    existing.updated_at = now;
                    existing.ttl_start = existing.ttl.map(|_| Instant::now());
                    (existing.version, ChangeKind::Updated)
                } else {
                    let version = current_version + 1;
                    let entry = BlackboardEntry {
                        key: key.clone(),
                        value,
                        owner: agent.clone(),
                        version,
                        created_at: now,
                        updated_at: now,
                        namespace: namespace.clone(),
                        ttl: None,
                        ttl_start: None,
                    };
                    shard.insert(key.clone(), entry);
                    self.tombstones.remove(&key);
                    (version, ChangeKind::Created)
                };
                let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
                Ok((version, kind, seq))
            });

        let (version, kind, seq) = cas_result?;

        let change = BlackboardChange {
            key: key.clone(),
            namespace,
            kind,
            agent,
            version,
            seq,
            value: None,
            ttl_ms: None,
        };
        self.publish_change(change, Some(value_for_journal), None);
        debug!(key = %key, version, "blackboard CAS write");
        Ok(version)
    }

    pub fn read(&self, key: &str) -> Option<BlackboardEntry> {
        let key = ensure_session_scoped_key(key.to_string());
        self.entries.with_shard(&key, |shard| {
            shard.get(&key).and_then(|e| {
                if e.is_expired() {
                    None
                } else {
                    Some(e.clone())
                }
            })
        })
    }

    pub fn get_value(&self, key: &str) -> Option<serde_json::Value> {
        self.read(key).map(|e| e.value)
    }

    pub fn delete(&self, key: &str, agent: &str) -> bool {
        let key = ensure_session_scoped_key(key.to_string());
        let removed_opt = self.entries.with_shard_mut(&key, |shard| {
            shard.remove(&key).map(|removed| {
                let version = removed.version + 1;
                self.tombstones.insert(
                    key.clone(),
                    Tombstone {
                        version,
                        deleted_at: Instant::now(),
                    },
                );
                let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
                (removed.namespace, version, seq)
            })
        });
        if let Some((namespace, version, seq)) = removed_opt {
            let change = BlackboardChange {
                key: key.clone(),
                namespace,
                kind: ChangeKind::Deleted,
                agent: agent.to_string(),
                version,
                seq,
                value: None,
                ttl_ms: None,
            };
            self.publish_change(change, None, None);
            debug!(key = %key, "blackboard delete");
            true
        } else {
            false
        }
    }

    fn publish_change(
        &self,
        change: BlackboardChange,
        journal_value: Option<serde_json::Value>,
        journal_ttl: Option<Duration>,
    ) {
        if let Some(journal) = self.journal.as_ref() {
            let mut record = change.clone();
            record.value = journal_value;
            record.ttl_ms = journal_ttl.map(|d| d.as_millis() as u64);
            journal.append(&record);
        }
        coordination_metrics::incr_blackboard_published();
        let _ = self.change_sender.send(change);

        if self.write_count.fetch_add(1, Ordering::Relaxed) + 1 >= EVICT_EVERY_N_WRITES {
            self.write_count.store(0, Ordering::Relaxed);
            self.evict_expired();
        }
    }

    pub fn next_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    pub fn keys_in_namespace(&self, namespace: &str) -> Vec<String> {
        self.entries
            .values_snapshot()
            .into_iter()
            .filter(|e| e.namespace == namespace && !e.is_expired())
            .map(|e| e.key)
            .collect()
    }

    pub fn entries_in_namespace(&self, namespace: &str) -> Vec<BlackboardEntry> {
        self.entries
            .values_snapshot()
            .into_iter()
            .filter(|e| e.namespace == namespace && !e.is_expired())
            .collect()
    }

    pub fn namespaces(&self) -> Vec<String> {
        let mut ns: Vec<String> = self
            .entries
            .values_snapshot()
            .into_iter()
            .map(|e| e.namespace)
            .collect();
        ns.sort();
        ns.dedup();
        ns
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BlackboardChange> {
        self.change_sender.subscribe()
    }

    pub fn subscribe_from(&self, since: u64) -> BlackboardStream {
        let receiver = self.change_sender.subscribe();
        let current = self.next_seq();
        let (backlog, needs_backfill, pending_gap) = match self.journal.as_ref() {
            Some(journal) => {
                let slice = journal.replay_since(since);
                if slice.complete {
                    (slice.changes, false, 0)
                } else {
                    (Vec::new(), true, current.saturating_sub(since))
                }
            }
            None => (Vec::new(), false, current.saturating_sub(since)),
        };
        if !backlog.is_empty() {
            coordination_metrics::incr_blackboard_replayed(backlog.len() as u64);
        }
        BlackboardStream {
            receiver,
            journal: self.journal.clone(),
            cursor: since,
            backlog: backlog.into_iter(),
            needs_backfill,
            pending_gap,
        }
    }

    pub fn journal(&self) -> Option<Arc<BlackboardJournal>> {
        self.journal.clone()
    }

    pub fn evict_expired(&self) -> usize {
        let removed = self.entries.retain(|_, e| !e.is_expired());
        let tombstones_removed = self.tombstones.retain(|_, t| !t.is_expired());
        if removed > 0 || tombstones_removed > 0 {
            debug!(
                removed,
                tombstones_removed,
                "evicted expired blackboard entries and tombstones"
            );
        }
        removed
    }

    pub fn clear(&self) {
        self.entries.clear();
        self.tombstones.clear();
    }

    pub fn len(&self) -> usize {
        self.entries
            .values_snapshot()
            .into_iter()
            .filter(|e| !e.is_expired())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn snapshot(&self) -> Vec<BlackboardEntry> {
        self.entries
            .values_snapshot()
            .into_iter()
            .filter(|e| !e.is_expired())
            .collect()
    }
}

impl Default for Blackboard {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum BlackboardStreamItem {
    Change(BlackboardChange),
    Gap { missed: u64 },
}

pub struct BlackboardStream {
    receiver: broadcast::Receiver<BlackboardChange>,
    journal: Option<Arc<BlackboardJournal>>,
    cursor: u64,
    backlog: std::vec::IntoIter<BlackboardChange>,
    needs_backfill: bool,
    pending_gap: u64,
}

impl BlackboardStream {

    pub async fn recv(&mut self) -> Option<BlackboardStreamItem> {
        if self.needs_backfill {
            self.needs_backfill = false;
            let missed_hint = self.pending_gap;
            self.pending_gap = 0;
            if let Some(item) = self.replay_missed(missed_hint).await {
                return Some(item);
            }
        }
        if self.pending_gap > 0 {
            let missed = self.pending_gap;
            self.pending_gap = 0;
            return Some(self.emit_gap(missed));
        }
        if let Some(next) = self.backlog.next() {
            self.cursor = self.cursor.max(next.seq);
            coordination_metrics::incr_blackboard_delivered();
            return Some(BlackboardStreamItem::Change(next));
        }
        loop {
            match self.receiver.recv().await {
                Ok(change) => {
                    self.cursor = self.cursor.max(change.seq);
                    coordination_metrics::incr_blackboard_delivered();
                    return Some(BlackboardStreamItem::Change(change));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    coordination_metrics::incr_blackboard_lagged(n);
                    match self.replay_missed(n).await {
                        Some(item) => return Some(item),
                        None => continue,
                    }
                }
            }
        }
    }

    async fn replay_missed(&mut self, missed_hint: u64) -> Option<BlackboardStreamItem> {
        let Some(journal) = self.journal.clone() else {
            return Some(self.emit_gap(missed_hint));
        };
        let slice = journal.replay_since(self.cursor);
        if slice.complete {
            return self.deliver_replayed(slice.changes);
        }
        if !journal.persists() {
            return Some(self.emit_gap(missed_hint));
        }
        let cursor = self.cursor;
        let journal_for_read = journal.clone();
        let file_result =
            tokio::task::spawn_blocking(move || journal_for_read.read_file_since(cursor)).await;
        match file_result {
            Ok(Ok(mut merged)) => {
                merged.extend(slice.changes);
                merged.sort_by_key(|c| c.seq);
                merged.dedup_by_key(|c| c.seq);
                let mut expected = cursor + 1;
                for change in &merged {
                    if change.seq != expected {
                        return Some(self.emit_gap(missed_hint));
                    }
                    expected += 1;
                }
                self.deliver_replayed(merged)
            }
            Ok(Err(e)) => {
                warn!(
                    path = %journal.path().display(),
                    error = %e,
                    "blackboard journal file read failed during lag recovery"
                );
                Some(self.emit_gap(missed_hint))
            }
            Err(_) => Some(self.emit_gap(missed_hint)),
        }
    }

    fn deliver_replayed(
        &mut self,
        changes: Vec<BlackboardChange>,
    ) -> Option<BlackboardStreamItem> {
        if changes.is_empty() {
            return None;
        }
        coordination_metrics::incr_blackboard_replayed(changes.len() as u64);
        self.backlog = changes.into_iter();
        let next = self.backlog.next()?;
        self.cursor = self.cursor.max(next.seq);
        coordination_metrics::incr_blackboard_delivered();
        Some(BlackboardStreamItem::Change(next))
    }

    fn emit_gap(&self, missed: u64) -> BlackboardStreamItem {
        warn!(
            missed,
            cursor = self.cursor,
            "blackboard subscriber lost changes that cannot be replayed; consumers must re-read the keys they depend on"
        );
        BlackboardStreamItem::Gap { missed }
    }

    pub fn cursor(&self) -> u64 {
        self.cursor
    }
}

#[derive(Clone)]
pub struct BlackboardHandle {
    inner: Arc<Blackboard>,
}

impl BlackboardHandle {
    pub fn new(bb: Blackboard) -> Self {
        Self {
            inner: Arc::new(bb),
        }
    }

    pub fn from_arc(arc: Arc<Blackboard>) -> Self {
        Self { inner: arc }
    }

    pub fn inner(&self) -> &Blackboard {
        &self.inner
    }

    pub fn write(
        &self,
        key: impl Into<String>,
        value: serde_json::Value,
        agent: impl Into<String>,
        namespace: impl Into<String>,
    ) -> u64 {
        self.inner.write(key, value, agent, namespace)
    }

    pub fn read(&self, key: &str) -> Option<BlackboardEntry> {
        self.inner.read(key)
    }

    pub fn delete(&self, key: &str, agent: &str) -> bool {
        self.inner.delete(key, agent)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BlackboardChange> {
        self.inner.subscribe()
    }

    pub fn subscribe_from(&self, since: u64) -> BlackboardStream {
        self.inner.subscribe_from(since)
    }

    pub fn compare_and_swap(
        &self,
        key: impl Into<String>,
        value: serde_json::Value,
        agent: impl Into<String>,
        namespace: impl Into<String>,
        expected_version: u64,
    ) -> Result<u64, BlackboardError> {
        self.inner
            .compare_and_swap(key, value, agent, namespace, expected_version)
    }
}

impl From<Blackboard> for BlackboardHandle {
    fn from(bb: Blackboard) -> Self {
        Self::new(bb)
    }
}
