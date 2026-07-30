// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::crdt::{CrdtError, CrdtUpdate, Document, RemoteApplyOutcome};
use crate::apply_model::edit_op::EditOp;

struct DocEntry {
    doc: Mutex<Document>,
    last_touched: Mutex<Instant>,
}

static DOCS: Lazy<DashMap<PathBuf, Arc<DocEntry>>> = Lazy::new(DashMap::new);

static PROCESS_SITE: Lazy<String> =
    Lazy::new(|| format!("process-{}-{}", std::process::id(), uuid::Uuid::new_v4()));

static PROCESS_NONCE: Lazy<String> = Lazy::new(|| {
    let full = uuid::Uuid::new_v4().simple().to_string();
    full[..8].to_string()
});

const MAX_LOG_OPS: usize = 512;
const MAX_CAS_ATTEMPTS: usize = 8;
const MAX_CACHED_DOCS: usize = 512;

static EXPERIMENTAL_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn warn_experimental_once() {
    if !EXPERIMENTAL_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::warn!(
            target: "crdt.coordination",
            "crdt-coordination is EXPERIMENTAL and PROCESS-LOCAL: it coordinates sessions and \
             workers inside this one process via the in-process blackboard, providing \
             last-writer-wins convergence with a pre-image guard plus a re-read-and-retry \
             conflict path. It is NOT operational-transform conflict resolution and does NOT \
             synchronize across processes or devices; cross-process write safety comes from the \
             OS advisory file locks in the workspace resource manager. Remote ops whose \
             pre-image no longer matches the local text are rejected (never blindly applied) \
             and surface as re-read-and-retry errors."
        );
    }
}

pub fn coordination_identity() -> (String, String) {
    match crate::session::current_session_context() {
        Some(ctx) => {
            let site = if ctx.session_id.is_empty() {
                tracing::debug!(
                    target: "crdt.coordination",
                    "session context has an empty session id; using the process-level crdt site"
                );
                PROCESS_SITE.clone()
            } else {
                format!("{}#{}", ctx.session_id, &*PROCESS_NONCE)
            };
            (site, ctx.workspace_key)
        }
        None => {
            tracing::debug!(
                target: "crdt.coordination",
                "no session context available; using the process-level crdt site"
            );
            (PROCESS_SITE.clone(), String::new())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeqOp {
    seq: u64,
    #[serde(flatten)]
    op: CrdtUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrdtSnapshot {
    seq: u64,
    site: String,
    clock: u64,
    text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CrdtLog {
    #[serde(default)]
    first_seq: u64,
    #[serde(default)]
    next_seq: u64,
    #[serde(default)]
    ops: Vec<SeqOp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot: Option<CrdtSnapshot>,
}

impl CrdtLog {
    fn ensure_initialized(&mut self) {
        if self.next_seq == 0 {
            self.next_seq = 1;
            self.first_seq = 1;
        }
    }

    fn append(&mut self, new_ops: &[CrdtUpdate]) {
        if new_ops.is_empty() {
            return;
        }
        self.ensure_initialized();
        for op in new_ops {
            let dup = self
                .ops
                .iter()
                .any(|s| s.op.site() == op.site() && s.op.clock() == op.clock());
            if dup {
                continue;
            }
            self.ops.push(SeqOp {
                seq: self.next_seq,
                op: op.clone(),
            });
            self.next_seq += 1;
        }
        self.first_seq = self.ops.first().map(|s| s.seq).unwrap_or(self.next_seq);
    }

    fn fold_into_snapshot(&mut self, site: &str, clock: u64, text: &str) -> u64 {
        self.ensure_initialized();
        let seq = self.next_seq;
        self.next_seq += 1;
        self.snapshot = Some(CrdtSnapshot {
            seq,
            site: site.to_string(),
            clock,
            text: text.to_string(),
        });
        self.ops.clear();
        self.first_seq = self.next_seq;
        seq
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DocMeta {
    #[serde(default)]
    consumed_seq: u64,
    #[serde(default)]
    seen: HashMap<String, u64>,
}

fn canonical_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn scoped_crdt_key(prefix: &str, path: &Path, workspace_key: &str) -> String {
    let canon = canonical_key(path);
    let normalized = canon.to_string_lossy().replace('\\', "/");
    if workspace_key.is_empty() {
        format!("__global__::{prefix}/{normalized}")
    } else {
        format!("ws::{workspace_key}::{prefix}/{normalized}")
    }
}

fn blackboard_key(path: &Path, workspace_key: &str) -> String {
    scoped_crdt_key("crdt", path, workspace_key)
}

fn meta_key(path: &Path, workspace_key: &str) -> String {
    scoped_crdt_key("crdt-meta", path, workspace_key)
}

fn load_meta(doc: &mut Document, workspace_key: &str) {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return;
    };
    let key = meta_key(doc.path(), workspace_key);
    let Some(entry) = rt.blackboard.read(&key) else {
        return;
    };
    if let Ok(meta) = serde_json::from_value::<DocMeta>(entry.value) {
        doc.restore_meta(meta.consumed_seq, meta.seen);
    }
}

fn persist_meta(doc: &Document, site: &str, workspace_key: &str) {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return;
    };
    let key = meta_key(doc.path(), workspace_key);
    let meta = DocMeta {
        consumed_seq: doc.consumed_seq(),
        seen: doc.seen_clocks().clone(),
    };
    if let Ok(value) = serde_json::to_value(&meta) {
        rt.blackboard.write(key, value, site, "crdt");
    }
}

fn evict_docs_if_needed() {
    if DOCS.len() <= MAX_CACHED_DOCS {
        return;
    }
    let mut candidates: Vec<(PathBuf, Instant)> = Vec::new();
    for entry in DOCS.iter() {
        if Arc::strong_count(entry.value()) > 1 {
            continue;
        }
        let Some(doc) = entry.value().doc.try_lock() else {
            continue;
        };
        if doc.has_pending() || doc.needs_snapshot_publish() {
            continue;
        }
        drop(doc);
        candidates.push((entry.key().clone(), *entry.value().last_touched.lock()));
    }
    let excess = DOCS.len().saturating_sub(MAX_CACHED_DOCS);
    if excess == 0 {
        return;
    }
    candidates.sort_by_key(|(_, touched)| *touched);
    for (key, _) in candidates.into_iter().take(excess) {
        DOCS.remove(&key);
    }
}

fn get_or_open(path: &Path, workspace_key: &str) -> Result<Arc<DocEntry>, CrdtError> {
    let key = canonical_key(path);
    let slot = match DOCS.entry(key.clone()) {
        dashmap::mapref::entry::Entry::Occupied(occupied) => {
            let slot = occupied.get().clone();
            drop(occupied);
            *slot.last_touched.lock() = Instant::now();
            slot
        }
        dashmap::mapref::entry::Entry::Vacant(vacant) => {
            let mut doc = Document::from_path(&key)?;
            load_meta(&mut doc, workspace_key);
            let slot = Arc::new(DocEntry {
                doc: Mutex::new(doc),
                last_touched: Mutex::new(Instant::now()),
            });
            vacant.insert(slot.clone());
            slot
        }
    };
    evict_docs_if_needed();
    Ok(slot)
}

pub fn observe_after_disk_write(
    op: &EditOp,
    site: &str,
    workspace_key: &str,
) -> Result<(), CrdtError> {
    match op {
        EditOp::Replace { .. } | EditOp::Insert { .. } | EditOp::Delete { .. } => {}
        _ => return Ok(()),
    }
    warn_experimental_once();
    let path = op.primary_path();
    let slot = get_or_open(path, workspace_key)?;
    let mut doc = slot.doc.lock();
    doc.observe_local(op, site)?;
    publish_pending_locked(&mut doc, site, workspace_key);
    persist_meta(&doc, site, workspace_key);
    Ok(())
}

pub fn mark_needs_resync(path: &Path, site: &str, workspace_key: &str) {
    let slot = match get_or_open(path, workspace_key) {
        Ok(slot) => slot,
        Err(e) => {
            tracing::warn!(
                target: "crdt.coordination",
                path = %path.display(),
                error = %e,
                "failed to open crdt document while marking it for full snapshot resync"
            );
            return;
        }
    };
    let mut doc = slot.doc.lock();
    doc.mark_needs_snapshot_publish();
    publish_pending_locked(&mut doc, site, workspace_key);
    persist_meta(&doc, site, workspace_key);
}

pub fn merge_remote_for_path(
    path: &Path,
    update: &[u8],
    site: &str,
    workspace_key: &str,
) -> Result<bool, CrdtError> {
    if update.is_empty() {
        return Ok(false);
    }
    let slot = get_or_open(path, workspace_key)?;
    let mut doc = slot.doc.lock();
    if doc.ensure_disk_fresh().unwrap_or(false) {
        tracing::debug!(
            target: "crdt.coordination",
            path = %path.display(),
            "crdt document base was stale relative to disk; resynced before merging remote ops"
        );
    }
    let updates: Vec<CrdtUpdate> = match serde_json::from_slice::<Vec<CrdtUpdate>>(update) {
        Ok(batch) => batch,
        Err(_) => {
            let single: CrdtUpdate =
                serde_json::from_slice(update).map_err(|e| CrdtError::Decode(format!("{e}")))?;
            vec![single]
        }
    };
    let mut applied = false;
    let mut conflicted = false;
    for parsed in updates {
        match doc.apply_remote_update(parsed, site) {
            RemoteApplyOutcome::Applied => applied = true,
            RemoteApplyOutcome::Conflict(reason) => {
                conflicted = true;
                tracing::warn!(
                    target: "crdt.coordination",
                    path = %path.display(),
                    reason,
                    "rejected remote crdt op: pre-image no longer matches local text"
                );
            }
            RemoteApplyOutcome::Duplicate | RemoteApplyOutcome::OwnOp => {}
        }
    }
    if applied {
        doc.save()?;
    }
    if conflicted {
        doc.resync_from_disk()?;
    }
    if applied || conflicted {
        persist_meta(&doc, site, workspace_key);
    }
    Ok(applied || conflicted)
}

pub fn pull_remote_before_edit(path: &Path, site: &str, workspace_key: &str) -> bool {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return false;
    };
    let key = blackboard_key(path, workspace_key);
    let Some(entry) = rt.blackboard.read(&key) else {
        return false;
    };
    let Ok(log) = serde_json::from_value::<CrdtLog>(entry.value.clone()) else {
        let Ok(bytes) = serde_json::to_vec(&entry.value) else {
            return false;
        };
        return merge_remote_for_path(path, &bytes, site, workspace_key).unwrap_or(false);
    };

    let Ok(slot) = get_or_open(path, workspace_key) else {
        return false;
    };
    let mut doc = slot.doc.lock();
    if doc.ensure_disk_fresh().unwrap_or(false) {
        tracing::debug!(
            target: "crdt.coordination",
            path = %path.display(),
            "crdt document base was stale relative to disk; resynced before applying pulled ops"
        );
    }
    let mut consumed = doc.consumed_seq();

    let newest = log.next_seq.saturating_sub(1);
    if newest <= consumed {
        return false;
    }

    let mut meta_dirty = false;

    if let Some(snap) = log.snapshot.as_ref() {
        if snap.seq > consumed {
            let _ = doc.resync_from_disk();
            doc.note_seen(&snap.site, snap.clock);
            doc.set_consumed_seq(snap.seq);
            consumed = snap.seq;
            meta_dirty = true;
        }
    }

    if consumed > 0 && log.first_seq > consumed + 1 && log.ops.iter().any(|s| s.seq > consumed) {
        tracing::warn!(
            target: "crdt.coordination",
            path = %path.display(),
            consumed,
            first_seq = log.first_seq,
            "crdt log gap detected; forcing re-read instead of partial merge"
        );
        crate::observability::coordination_metrics::incr_crdt_conflicts(1);
        let _ = doc.resync_from_disk();
        doc.set_consumed_seq(newest);
        persist_meta(&doc, site, workspace_key);
        return true;
    }

    let mut applied = false;
    let mut conflicted = false;
    let mut max_seq = consumed;
    for seq_op in log.ops.into_iter().filter(|s| s.seq > consumed) {
        max_seq = max_seq.max(seq_op.seq);
        match doc.apply_remote_update(seq_op.op, site) {
            RemoteApplyOutcome::Applied => applied = true,
            RemoteApplyOutcome::Conflict(reason) => {
                conflicted = true;
                tracing::warn!(
                    target: "crdt.coordination",
                    path = %path.display(),
                    seq = seq_op.seq,
                    reason,
                    "rejected remote crdt op: pre-image no longer matches local text"
                );
            }
            RemoteApplyOutcome::Duplicate | RemoteApplyOutcome::OwnOp => {}
        }
    }
    if max_seq > consumed {
        doc.set_consumed_seq(max_seq);
        meta_dirty = true;
    }
    if applied {
        if let Err(e) = doc.save() {
            tracing::warn!(
                target: "crdt.coordination",
                path = %path.display(),
                error = %e,
                "failed to persist merged remote crdt ops"
            );
        }
    }
    if conflicted {
        let _ = doc.resync_from_disk();
    }
    if meta_dirty {
        persist_meta(&doc, site, workspace_key);
    }
    applied || conflicted
}

fn publish_pending_locked(doc: &mut Document, site: &str, workspace_key: &str) {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return;
    };
    let wants_snapshot = doc.needs_snapshot_publish();
    let pending = doc.pending_updates().unwrap_or_default();
    if pending.is_empty() && !wants_snapshot {
        return;
    }
    let key = blackboard_key(doc.path(), workspace_key);

    let mut recreate_from = 0u64;
    for _ in 0..MAX_CAS_ATTEMPTS {
        let existing = rt.blackboard.read(&key);
        let (mut log, expected_version) = match existing {
            Some(entry) => {
                let log =
                    serde_json::from_value::<CrdtLog>(entry.value.clone()).unwrap_or_default();
                (log, entry.version)
            }
            None => (CrdtLog::default(), recreate_from),
        };
        let snap_seq = if wants_snapshot {
            Some(log.fold_into_snapshot(site, doc.current_clock(), doc.current_text()))
        } else {
            log.append(&pending);
            if log.ops.len() > MAX_LOG_OPS {
                Some(log.fold_into_snapshot(site, doc.current_clock(), doc.current_text()))
            } else {
                None
            }
        };
        let Ok(value) = serde_json::to_value(&log) else {
            return;
        };
        match rt.blackboard.compare_and_swap(
            key.clone(),
            value,
            site.to_string(),
            "crdt",
            expected_version,
        ) {
            Ok(_) => {
                doc.mark_exported(pending.len());
                doc.clear_needs_snapshot_publish();
                if let Some(seq) = snap_seq {
                    doc.set_consumed_seq(doc.consumed_seq().max(seq));
                }
                return;
            }
            Err(crate::memory::blackboard::BlackboardError::VersionConflict {
                actual, ..
            }) => {
                recreate_from = actual;
                continue;
            }
            Err(_) => continue,
        }
    }
    tracing::warn!(
        target: "crdt.coordination",
        path = %doc.path().display(),
        "crdt publish lost CAS race repeatedly; ops stay queued for the next publish"
    );
}

pub fn flush_path(path: &Path) -> Result<(), CrdtError> {
    let key = canonical_key(path);
    if let Some(slot) = DOCS.get(&key) {
        slot.doc.lock().save()?;
    }
    Ok(())
}

pub fn invalidate(path: &Path) {
    let key = canonical_key(path);
    DOCS.remove(&key);
}
