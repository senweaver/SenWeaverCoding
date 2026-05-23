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

use super::sharded_map::ShardedMap;
use crate::observability::coordination_metrics;

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

pub struct BlackboardJournal {
    file: parking_lot::Mutex<std::fs::File>,
    path: PathBuf,
    in_memory: parking_lot::Mutex<Vec<BlackboardChange>>,
}

impl BlackboardJournal {
    pub fn open(dir: &Path, session: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{session}.jsonl"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        let mut in_memory: Vec<BlackboardChange> = Vec::new();
        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(rec) = serde_json::from_str::<BlackboardChange>(line) {
                    in_memory.push(rec);
                }
            }
        }

        Ok(Self {
            file: parking_lot::Mutex::new(file),
            path,
            in_memory: parking_lot::Mutex::new(in_memory),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, change: &BlackboardChange) {
        if let Ok(mut line) = serde_json::to_string(change) {
            line.push('\n');
            let mut file = self.file.lock();
            use std::io::Write;
            if let Err(e) = file.write_all(line.as_bytes()) {
                warn!("blackboard journal append failed: {e}");
            }
        }
        self.in_memory.lock().push(change.clone());
    }

    pub fn replay_since(&self, since: u64) -> Vec<BlackboardChange> {
        self.in_memory
            .lock()
            .iter()
            .filter(|c| c.seq > since)
            .cloned()
            .collect()
    }
}

pub struct Blackboard {
    entries: ShardedMap<BlackboardEntry>,
    change_sender: broadcast::Sender<BlackboardChange>,
    seq: AtomicU64,
    journal: Option<Arc<BlackboardJournal>>,
}

impl Blackboard {

    pub fn new() -> Self {
        let (change_sender, _rx) = broadcast::channel(CHANGE_CHANNEL_CAPACITY);
        Self {
            entries: ShardedMap::new(),
            change_sender,
            seq: AtomicU64::new(0),
            journal: None,
        }
    }

    pub fn with_persistence(journal_dir: Option<PathBuf>, session: impl AsRef<str>) -> Self {
        let mut bb = Self::new();
        if let Some(dir) = journal_dir {
            match BlackboardJournal::open(&dir, session.as_ref()) {
                Ok(journal) => {

                    let last_seq = journal
                        .in_memory
                        .lock()
                        .iter()
                        .map(|c| c.seq)
                        .max()
                        .unwrap_or(0);
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
        let key = key.into();
        let agent = agent.into();
        let namespace = namespace.into();
        let now = Utc::now();

        let (version, kind) = self.entries.with_shard_mut(&key, |shard| {
            if let Some(existing) = shard.get_mut(&key) {
                existing.value = value;
                existing.owner = agent.clone();
                existing.version += 1;
                existing.updated_at = now;
                existing.ttl_start = existing.ttl.map(|_| Instant::now());
                (existing.version, ChangeKind::Updated)
            } else {
                let entry = BlackboardEntry {
                    key: key.clone(),
                    value,
                    owner: agent.clone(),
                    version: 1,
                    created_at: now,
                    updated_at: now,
                    namespace: namespace.clone(),
                    ttl: None,
                    ttl_start: None,
                };
                shard.insert(key.clone(), entry);
                (1, ChangeKind::Created)
            }
        });

        let change = BlackboardChange {
            key: key.clone(),
            namespace,
            kind,
            agent,
            version,
            seq: 0,
        };
        self.publish_change(change);
        debug!(key = %key, version, "blackboard write");
        version
    }

    pub fn write_with_ttl(
        &self,
        key: impl Into<String>,
        value: serde_json::Value,
        agent: impl Into<String>,
        namespace: impl Into<String>,
        ttl: Duration,
    ) -> u64 {
        let key_str: String = key.into();
        let version = self.write(key_str.clone(), value, agent, namespace);

        self.entries.with_shard_mut(&key_str, |shard| {
            if let Some(entry) = shard.get_mut(&key_str) {
                entry.ttl = Some(ttl);
                entry.ttl_start = Some(Instant::now());
            }
        });
        version
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
        let key = key.into();
        let agent = agent.into();
        let namespace = namespace.into();
        let now = Utc::now();

        let cas_result: Result<(u64, ChangeKind), BlackboardError> =
            self.entries.with_shard_mut(&key, |shard| {
                let current_version = shard.get(&key).map(|e| e.version).unwrap_or(0);
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
                    let entry = BlackboardEntry {
                        key: key.clone(),
                        value,
                        owner: agent.clone(),
                        version: 1,
                        created_at: now,
                        updated_at: now,
                        namespace: namespace.clone(),
                        ttl: None,
                        ttl_start: None,
                    };
                    shard.insert(key.clone(), entry);
                    (1, ChangeKind::Created)
                };
                Ok((version, kind))
            });

        let (version, kind) = cas_result?;

        let change = BlackboardChange {
            key: key.clone(),
            namespace,
            kind,
            agent,
            version,
            seq: 0,
        };
        self.publish_change(change);
        debug!(key = %key, version, "blackboard CAS write");
        Ok(version)
    }

    pub fn read(&self, key: &str) -> Option<BlackboardEntry> {
        self.entries.with_shard(key, |shard| {
            shard.get(key).and_then(|e| {
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
        let removed_opt = self.entries.remove(key);
        if let Some(removed) = removed_opt {
            let change = BlackboardChange {
                key: key.to_string(),
                namespace: removed.namespace,
                kind: ChangeKind::Deleted,
                agent: agent.to_string(),
                version: removed.version + 1,
                seq: 0,
            };
            self.publish_change(change);
            debug!(key = %key, "blackboard delete");
            true
        } else {
            false
        }
    }

    fn publish_change(&self, mut change: BlackboardChange) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        change.seq = seq;
        if let Some(journal) = self.journal.as_ref() {
            journal.append(&change);
        }
        coordination_metrics::incr_blackboard_published();
        let _ = self.change_sender.send(change);
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
        let backlog = self
            .journal
            .as_ref()
            .map(|j| j.replay_since(since))
            .unwrap_or_default();
        if !backlog.is_empty() {
            coordination_metrics::incr_blackboard_replayed(backlog.len() as u64);
        }
        BlackboardStream {
            receiver,
            journal: self.journal.clone(),
            cursor: since,
            backlog: backlog.into_iter(),
        }
    }

    pub fn journal(&self) -> Option<Arc<BlackboardJournal>> {
        self.journal.clone()
    }

    pub fn evict_expired(&self) -> usize {
        let removed = self.entries.retain(|_, e| !e.is_expired());
        if removed > 0 {
            debug!(removed, "evicted expired blackboard entries");
        }
        removed
    }

    pub fn clear(&self) {
        self.entries.clear();
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

pub struct BlackboardStream {
    receiver: broadcast::Receiver<BlackboardChange>,
    journal: Option<Arc<BlackboardJournal>>,
    cursor: u64,
    backlog: std::vec::IntoIter<BlackboardChange>,
}

impl BlackboardStream {

    pub async fn recv(&mut self) -> Option<BlackboardChange> {
        if let Some(next) = self.backlog.next() {
            self.cursor = self.cursor.max(next.seq);
            coordination_metrics::incr_blackboard_delivered();
            return Some(next);
        }
        loop {
            match self.receiver.recv().await {
                Ok(change) => {
                    self.cursor = self.cursor.max(change.seq);
                    coordination_metrics::incr_blackboard_delivered();
                    return Some(change);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    coordination_metrics::incr_blackboard_lagged(n);
                    if let Some(journal) = self.journal.as_ref() {
                        let missed = journal.replay_since(self.cursor);
                        if !missed.is_empty() {
                            coordination_metrics::incr_blackboard_replayed(missed.len() as u64);
                            self.backlog = missed.into_iter();
                            if let Some(next) = self.backlog.next() {
                                self.cursor = self.cursor.max(next.seq);
                                coordination_metrics::incr_blackboard_delivered();
                                return Some(next);
                            }
                        }
                    }

                    continue;
                }
            }
        }
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
