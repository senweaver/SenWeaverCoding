// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared Blackboard — collaborative state for multi-agent coordination.
//!
//! The blackboard is a shared key-value store where multiple agents can
//! read, write, and watch for changes. It supports:
//!
//! - **Typed entries** with versioning and ownership tracking
//! - **Watch notifications** via tokio broadcast for reactive updates
//! - **Conflict detection** via optimistic concurrency (version checks)
//! - **Namespaced sections** for organizing collaborative data
//! - **TTL-based expiration** for ephemeral shared state

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::debug;

/// A single entry on the blackboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardEntry {
    /// The key for this entry.
    pub key: String,
    /// The value (JSON for flexibility).
    pub value: serde_json::Value,
    /// Which agent last wrote this entry.
    pub owner: String,
    /// Monotonically increasing version for conflict detection.
    pub version: u64,
    /// When this entry was created.
    pub created_at: DateTime<Utc>,
    /// When this entry was last updated.
    pub updated_at: DateTime<Utc>,
    /// Optional namespace for grouping.
    pub namespace: String,
    /// Optional TTL: entry expires after this duration from last update.
    #[serde(skip)]
    pub ttl: Option<Duration>,
    /// When the TTL clock started (last update instant).
    #[serde(skip)]
    pub ttl_start: Option<Instant>,
}

impl BlackboardEntry {
    /// Check if this entry has expired.
    pub fn is_expired(&self) -> bool {
        if let (Some(ttl), Some(start)) = (self.ttl, self.ttl_start) {
            start.elapsed() >= ttl
        } else {
            false
        }
    }
}

/// Notification of a blackboard change.
#[derive(Debug, Clone)]
pub struct BlackboardChange {
    /// The key that changed.
    pub key: String,
    /// The namespace.
    pub namespace: String,
    /// What kind of change.
    pub kind: ChangeKind,
    /// Which agent made the change.
    pub agent: String,
    /// New version number.
    pub version: u64,
}

/// Kind of change to a blackboard entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Entry was created.
    Created,
    /// Entry was updated.
    Updated,
    /// Entry was deleted.
    Deleted,
}

/// Error from blackboard operations.
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

/// Capacity for the change notification channel.
const CHANGE_CHANNEL_CAPACITY: usize = 512;

/// Shared blackboard for multi-agent collaborative state.
pub struct Blackboard {
    entries: RwLock<HashMap<String, BlackboardEntry>>,
    change_sender: broadcast::Sender<BlackboardChange>,
}

impl Blackboard {
    /// Create a new empty blackboard.
    pub fn new() -> Self {
        let (change_sender, _rx) = broadcast::channel(CHANGE_CHANNEL_CAPACITY);
        Self {
            entries: RwLock::new(HashMap::new()),
            change_sender,
        }
    }

    /// Write a value to the blackboard (create or overwrite).
    ///
    /// Returns the new version number.
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

        let mut entries = self.entries.write();
        let (version, kind) = if let Some(existing) = entries.get_mut(&key) {
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
            entries.insert(key.clone(), entry);
            (1, ChangeKind::Created)
        };

        let change = BlackboardChange {
            key: key.clone(),
            namespace,
            kind,
            agent,
            version,
        };
        let _ = self.change_sender.send(change);
        debug!(key = %key, version, "blackboard write");
        version
    }

    /// Write with a TTL (entry auto-expires after the duration).
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

        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&key_str) {
            entry.ttl = Some(ttl);
            entry.ttl_start = Some(Instant::now());
        }
        version
    }

    /// Conditional write: only succeeds if current version matches `expected_version`.
    ///
    /// Use version 0 to require the key does NOT exist (create-only).
    ///
    /// This operation is atomic: the version check and write happen under
    /// a single write lock to prevent TOCTOU race conditions.
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

        let mut entries = self.entries.write();

        let current_version = entries.get(&key).map(|e| e.version).unwrap_or(0);
        if current_version != expected_version {
            return Err(BlackboardError::VersionConflict {
                key,
                expected: expected_version,
                actual: current_version,
            });
        }

        // Perform the write atomically within the same lock
        let (version, kind) = if let Some(existing) = entries.get_mut(&key) {
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
            entries.insert(key.clone(), entry);
            (1, ChangeKind::Created)
        };
        drop(entries);

        let change = BlackboardChange {
            key: key.clone(),
            namespace,
            kind,
            agent,
            version,
        };
        let _ = self.change_sender.send(change);
        debug!(key = %key, version, "blackboard CAS write");
        Ok(version)
    }

    /// Read a value from the blackboard.
    pub fn read(&self, key: &str) -> Option<BlackboardEntry> {
        let entries = self.entries.read();
        entries.get(key).and_then(|e| {
            if e.is_expired() {
                None
            } else {
                Some(e.clone())
            }
        })
    }

    /// Read the raw JSON value for a key.
    pub fn get_value(&self, key: &str) -> Option<serde_json::Value> {
        self.read(key).map(|e| e.value)
    }

    /// Delete an entry. Returns true if it existed.
    pub fn delete(&self, key: &str, agent: &str) -> bool {
        let mut entries = self.entries.write();
        if let Some(removed) = entries.remove(key) {
            let change = BlackboardChange {
                key: key.to_string(),
                namespace: removed.namespace,
                kind: ChangeKind::Deleted,
                agent: agent.to_string(),
                version: removed.version + 1,
            };
            let _ = self.change_sender.send(change);
            debug!(key = %key, "blackboard delete");
            true
        } else {
            false
        }
    }

    /// List all keys in a namespace.
    pub fn keys_in_namespace(&self, namespace: &str) -> Vec<String> {
        self.entries
            .read()
            .values()
            .filter(|e| e.namespace == namespace && !e.is_expired())
            .map(|e| e.key.clone())
            .collect()
    }

    /// List all entries in a namespace.
    pub fn entries_in_namespace(&self, namespace: &str) -> Vec<BlackboardEntry> {
        self.entries
            .read()
            .values()
            .filter(|e| e.namespace == namespace && !e.is_expired())
            .cloned()
            .collect()
    }

    /// Get all namespaces that have entries.
    pub fn namespaces(&self) -> Vec<String> {
        let mut ns: Vec<String> = self
            .entries
            .read()
            .values()
            .map(|e| e.namespace.clone())
            .collect();
        ns.sort();
        ns.dedup();
        ns
    }

    /// Subscribe to change notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<BlackboardChange> {
        self.change_sender.subscribe()
    }

    /// Remove all expired entries. Returns count removed.
    pub fn evict_expired(&self) -> usize {
        let mut entries = self.entries.write();
        let before = entries.len();
        entries.retain(|_, e| !e.is_expired());
        let removed = before - entries.len();
        if removed > 0 {
            debug!(removed, "evicted expired blackboard entries");
        }
        removed
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.write().clear();
    }

    /// Total number of non-expired entries.
    pub fn len(&self) -> usize {
        self.entries
            .read()
            .values()
            .filter(|e| !e.is_expired())
            .count()
    }

    /// Whether the blackboard is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get a snapshot of all entries (for debugging / persistence).
    pub fn snapshot(&self) -> Vec<BlackboardEntry> {
        self.entries
            .read()
            .values()
            .filter(|e| !e.is_expired())
            .cloned()
            .collect()
    }
}

impl Default for Blackboard {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe handle to a shared blackboard.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_and_read() {
        let bb = Blackboard::new();
        let v = bb.write("task_status", json!("running"), "agent-1", "default");
        assert_eq!(v, 1);

        let entry = bb.read("task_status").unwrap();
        assert_eq!(entry.value, json!("running"));
        assert_eq!(entry.owner, "agent-1");
        assert_eq!(entry.version, 1);
    }

    #[test]
    fn write_updates_version() {
        let bb = Blackboard::new();
        bb.write("key", json!(1), "a1", "ns");
        let v = bb.write("key", json!(2), "a2", "ns");
        assert_eq!(v, 2);

        let entry = bb.read("key").unwrap();
        assert_eq!(entry.value, json!(2));
        assert_eq!(entry.owner, "a2");
    }

    #[test]
    fn compare_and_swap_success() {
        let bb = Blackboard::new();
        bb.write("key", json!(1), "a1", "ns");

        let v = bb.compare_and_swap("key", json!(2), "a2", "ns", 1).unwrap();
        assert_eq!(v, 2);
    }

    #[test]
    fn compare_and_swap_conflict() {
        let bb = Blackboard::new();
        bb.write("key", json!(1), "a1", "ns");

        let result = bb.compare_and_swap("key", json!(2), "a2", "ns", 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlackboardError::VersionConflict { .. }
        ));
    }

    #[test]
    fn compare_and_swap_create_only() {
        let bb = Blackboard::new();

        // version 0 means key must not exist
        let v = bb
            .compare_and_swap("new_key", json!("hello"), "a1", "ns", 0)
            .unwrap();
        assert_eq!(v, 1);

        // second attempt should fail (key now exists at version 1)
        let result = bb.compare_and_swap("new_key", json!("world"), "a2", "ns", 0);
        assert!(result.is_err());
    }

    #[test]
    fn delete_entry() {
        let bb = Blackboard::new();
        bb.write("key", json!(1), "a1", "ns");
        assert!(bb.delete("key", "a1"));
        assert!(bb.read("key").is_none());
        assert!(!bb.delete("key", "a1")); // already gone
    }

    #[test]
    fn namespace_operations() {
        let bb = Blackboard::new();
        bb.write("k1", json!(1), "a1", "ns1");
        bb.write("k2", json!(2), "a1", "ns1");
        bb.write("k3", json!(3), "a1", "ns2");

        let ns1_keys = bb.keys_in_namespace("ns1");
        assert_eq!(ns1_keys.len(), 2);

        let ns2_keys = bb.keys_in_namespace("ns2");
        assert_eq!(ns2_keys.len(), 1);

        let namespaces = bb.namespaces();
        assert!(namespaces.contains(&"ns1".to_string()));
        assert!(namespaces.contains(&"ns2".to_string()));
    }

    #[test]
    fn ttl_expiration() {
        let bb = Blackboard::new();
        bb.write_with_ttl(
            "ephemeral",
            json!("temp"),
            "a1",
            "ns",
            Duration::from_millis(1),
        );

        // Should exist immediately
        assert!(bb.read("ephemeral").is_some());

        // Wait for TTL to expire
        std::thread::sleep(Duration::from_millis(10));

        // Should be expired now
        assert!(bb.read("ephemeral").is_none());

        // Eviction should clean it up
        assert_eq!(bb.evict_expired(), 1);
    }

    #[test]
    fn change_notifications() {
        let bb = Blackboard::new();
        let mut rx = bb.subscribe();

        bb.write("key", json!(1), "a1", "ns");

        let change = rx.try_recv().unwrap();
        assert_eq!(change.key, "key");
        assert_eq!(change.kind, ChangeKind::Created);
        assert_eq!(change.version, 1);

        bb.write("key", json!(2), "a2", "ns");
        let change = rx.try_recv().unwrap();
        assert_eq!(change.kind, ChangeKind::Updated);
        assert_eq!(change.version, 2);

        bb.delete("key", "a1");
        let change = rx.try_recv().unwrap();
        assert_eq!(change.kind, ChangeKind::Deleted);
    }

    #[test]
    fn snapshot_and_len() {
        let bb = Blackboard::new();
        bb.write("a", json!(1), "a1", "ns");
        bb.write("b", json!(2), "a1", "ns");

        assert_eq!(bb.len(), 2);
        assert_eq!(bb.snapshot().len(), 2);
        assert!(!bb.is_empty());

        bb.clear();
        assert!(bb.is_empty());
    }

    #[test]
    fn handle_operations() {
        let handle = BlackboardHandle::new(Blackboard::new());
        handle.write("key", json!("value"), "agent", "ns");
        assert!(handle.read("key").is_some());
        assert!(handle.delete("key", "agent"));
    }
}
