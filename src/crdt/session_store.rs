// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::crdt::{CrdtError, CrdtUpdate, Document, RemoteApplyOutcome};
use crate::apply_model::edit_op::EditOp;

static DOCS: Lazy<DashMap<PathBuf, Arc<Mutex<Document>>>> = Lazy::new(DashMap::new);

static PROCESS_SITE: Lazy<String> = Lazy::new(|| format!("proc-{}", uuid::Uuid::new_v4()));

const MAX_LOG_OPS: usize = 512;
const MAX_CAS_ATTEMPTS: usize = 8;

static EXPERIMENTAL_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn warn_experimental_once() {
    if !EXPERIMENTAL_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::warn!(
            target: "crdt.coordination",
            "crdt-coordination is EXPERIMENTAL: it provides last-writer-wins convergence with a \
             pre-image guard plus a re-read-and-retry conflict path, NOT full operational-transform \
             conflict resolution. Remote ops whose pre-image no longer matches the local text are \
             rejected (never blindly applied) and surface as re-read-and-retry errors."
        );
    }
}

fn local_site() -> String {
    crate::session::current_session_context()
        .map(|c| c.session_id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| PROCESS_SITE.clone())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeqOp {
    seq: u64,
    #[serde(flatten)]
    op: CrdtUpdate,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CrdtLog {
    #[serde(default)]
    first_seq: u64,
    #[serde(default)]
    next_seq: u64,
    #[serde(default)]
    ops: Vec<SeqOp>,
}

impl CrdtLog {
    fn append(&mut self, new_ops: &[CrdtUpdate]) {
        if self.next_seq == 0 {
            self.next_seq = 1;
            self.first_seq = 1;
        }
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
        if self.ops.len() > MAX_LOG_OPS {
            let drop = self.ops.len() - MAX_LOG_OPS;
            self.ops.drain(..drop);
        }
        self.first_seq = self.ops.first().map(|s| s.seq).unwrap_or(self.next_seq);
    }
}

fn canonical_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn get_or_open(path: &Path) -> Result<Arc<Mutex<Document>>, CrdtError> {
    let key = canonical_key(path);
    if let Some(existing) = DOCS.get(&key) {
        return Ok(existing.clone());
    }
    let doc = Document::from_path(&key)?;
    let arc = Arc::new(Mutex::new(doc));
    DOCS.insert(key, arc.clone());
    Ok(arc)
}

pub fn observe_after_disk_write(op: &EditOp) -> Result<(), CrdtError> {
    match op {
        EditOp::Replace { .. } | EditOp::Insert { .. } | EditOp::Delete { .. } => {}
        _ => return Ok(()),
    }
    warn_experimental_once();
    let path = op.primary_path();
    let handle = get_or_open(path)?;
    let mut doc = handle.lock();
    doc.observe_local(op, &local_site())?;
    publish_pending_locked(&mut doc);
    Ok(())
}

pub fn merge_remote_for_path(path: &Path, update: &[u8]) -> Result<bool, CrdtError> {
    if update.is_empty() {
        return Ok(false);
    }
    let handle = get_or_open(path)?;
    let mut doc = handle.lock();
    let updates: Vec<CrdtUpdate> = match serde_json::from_slice::<Vec<CrdtUpdate>>(update) {
        Ok(batch) => batch,
        Err(_) => {
            let single: CrdtUpdate =
                serde_json::from_slice(update).map_err(|e| CrdtError::Decode(format!("{e}")))?;
            vec![single]
        }
    };
    let site = local_site();
    let mut applied = false;
    let mut conflicted = false;
    for parsed in updates {
        match doc.apply_remote_update(parsed, &site) {
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
    Ok(applied || conflicted)
}

pub fn pull_remote_before_edit(path: &Path) -> bool {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return false;
    };
    let key = blackboard_key(path);
    let Some(entry) = rt.blackboard.read(&key) else {
        return false;
    };
    let Ok(log) = serde_json::from_value::<CrdtLog>(entry.value.clone()) else {
        // Legacy or foreign payload shape: fall back to the raw batch decoder.
        let Ok(bytes) = serde_json::to_vec(&entry.value) else {
            return false;
        };
        return merge_remote_for_path(path, &bytes).unwrap_or(false);
    };

    let Ok(handle) = get_or_open(path) else {
        return false;
    };
    let mut doc = handle.lock();
    let consumed = doc.consumed_seq();

    if log.next_seq <= consumed + 1 && log.ops.iter().all(|s| s.seq <= consumed) {
        return false;
    }

    // First pull for this Document: the disk copy it was opened from already
    // reflects everything published so far, so fast-forward the cursor instead
    // of replaying history against fresh text (which would only trip the
    // pre-image guard and produce false "stale" reports).
    if consumed == 0 {
        let _ = doc.resync_from_disk();
        doc.set_consumed_seq(log.next_seq.saturating_sub(1));
        return false;
    }

    // Gap: ops between our cursor and the start of the retained window were
    // evicted before we saw them. Degrade safely: resync from disk, skip the
    // cursor forward, and report "stale" so byte-offset edits get re-read.
    let gap = log.first_seq > consumed + 1;
    if gap {
        tracing::warn!(
            target: "crdt.coordination",
            path = %path.display(),
            consumed,
            first_seq = log.first_seq,
            "crdt log gap detected; forcing re-read instead of partial merge"
        );
        crate::observability::coordination_metrics::incr_crdt_conflicts(1);
        let _ = doc.resync_from_disk();
        doc.set_consumed_seq(log.next_seq.saturating_sub(1));
        return true;
    }

    let site = local_site();
    let mut applied = false;
    let mut conflicted = false;
    let mut max_seq = consumed;
    for seq_op in log.ops.into_iter().filter(|s| s.seq > consumed) {
        max_seq = max_seq.max(seq_op.seq);
        match doc.apply_remote_update(seq_op.op, &site) {
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
    doc.set_consumed_seq(max_seq);
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
    applied || conflicted
}

fn blackboard_key(path: &Path) -> String {
    let rel = crate::session::current_session_context()
        .map(|c| PathBuf::from(c.workspace_dir))
        .and_then(|root| {
            let canon_root = root.canonicalize().unwrap_or(root);
            let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            canon_path
                .strip_prefix(&canon_root)
                .ok()
                .map(|p| p.to_path_buf())
        })
        .unwrap_or_else(|| path.to_path_buf());
    // Scope by workspace (not session) so every session/worker editing the same
    // working tree publishes and pulls the SAME key for a given file; a
    // session-scoped key made "remote merge" a self-referential no-op.
    crate::agent::multi_agent_runtime::workspace_scoped_key(&format!(
        "crdt/{}",
        rel.to_string_lossy().replace('\\', "/")
    ))
}

fn publish_pending_locked(doc: &mut Document) {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return;
    };
    let Some(pending) = doc.pending_updates() else {
        return;
    };
    let key = blackboard_key(doc.path());
    let agent = local_site();

    for _ in 0..MAX_CAS_ATTEMPTS {
        let existing = rt.blackboard.read(&key);
        let (mut log, expected_version) = match existing {
            Some(entry) => {
                let log = serde_json::from_value::<CrdtLog>(entry.value.clone())
                    .unwrap_or_default();
                (log, entry.version)
            }
            None => (CrdtLog::default(), 0),
        };
        log.append(&pending);
        let Ok(value) = serde_json::to_value(&log) else {
            return;
        };
        match rt
            .blackboard
            .compare_and_swap(key.clone(), value, agent.clone(), "crdt", expected_version)
        {
            Ok(_) => {
                // Do NOT fast-forward the consume cursor to next_seq-1 here: the
                // freshly-read log may contain external ops appended between our
                // pull and this publish, and skipping them would silently diverge.
                // Leave the cursor; the next pull re-scans and `apply_remote_update`
                // classifies our own ops as OwnOp (deduped, clock advanced), so we
                // never re-apply what we published while still catching externals.
                doc.mark_exported(pending.len());
                return;
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
    if let Some(handle) = DOCS.get(&key) {
        handle.lock().save()?;
    }
    Ok(())
}

pub fn invalidate(path: &Path) {
    let key = canonical_key(path);
    DOCS.remove(&key);
}
