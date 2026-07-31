// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::apply_model::edit_op::EditOp;
use crate::observability::coordination_metrics;

const MAX_HISTORY_ENTRIES: usize = 10_000;
const MAX_OUTBOX_ENTRIES: usize = 4_096;
const CONTEXT_WINDOW_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CrdtUpdate {

    Replace {
        clock: u64,
        #[serde(default)]
        site: String,
        path: String,
        start: usize,
        end: usize,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pre_hash: Option<String>,
    },

    Insert {
        clock: u64,
        #[serde(default)]
        site: String,
        path: String,
        at: usize,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pre_hash: Option<String>,
    },

    Delete {
        clock: u64,
        #[serde(default)]
        site: String,
        path: String,
        start: usize,
        end: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pre_hash: Option<String>,
    },
}

impl CrdtUpdate {
    pub fn clock(&self) -> u64 {
        match self {
            CrdtUpdate::Replace { clock, .. }
            | CrdtUpdate::Insert { clock, .. }
            | CrdtUpdate::Delete { clock, .. } => *clock,
        }
    }

    pub fn site(&self) -> &str {
        match self {
            CrdtUpdate::Replace { site, .. }
            | CrdtUpdate::Insert { site, .. }
            | CrdtUpdate::Delete { site, .. } => site,
        }
    }

    pub fn payload_len(&self) -> usize {
        match self {
            CrdtUpdate::Replace { text, .. } | CrdtUpdate::Insert { text, .. } => text.len(),
            CrdtUpdate::Delete { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteApplyOutcome {
    Applied,
    Duplicate,
    OwnOp,
    Conflict(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum CrdtError {

    #[error("crdt io: {0}")]
    Io(#[from] std::io::Error),

    #[error("crdt unsupported edit-op: {0}")]
    UnsupportedOp(String),

    #[error("crdt decode: {0}")]
    Decode(String),
}

pub fn region_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    hex::encode(&digest[..16])
}

fn snap_left(text: &str, mut i: usize) -> usize {
    if i > text.len() {
        i = text.len();
    }
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn snap_right(text: &str, mut i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn window_before(text: &str, before_end: usize) -> &str {
    let be = snap_left(text, before_end);
    let bs = snap_left(text, be.saturating_sub(CONTEXT_WINDOW_BYTES));
    &text[bs..be]
}

fn window_after(text: &str, after_start: usize) -> &str {
    let a_start = snap_right(text, after_start.min(text.len()));
    let a_end = snap_right(
        text,
        after_start
            .saturating_add(CONTEXT_WINDOW_BYTES)
            .min(text.len()),
    );
    if a_start < a_end {
        &text[a_start..a_end]
    } else {
        ""
    }
}

pub fn context_pair_hash(text: &str, before_end: usize, after_start: usize) -> String {
    let before = window_before(text, before_end);
    let after = window_after(text, after_start);
    let mut buf = String::with_capacity(before.len() + after.len() + 1);
    buf.push_str(before);
    buf.push('\u{1}');
    buf.push_str(after);
    region_hash(&buf)
}

pub fn replace_context_hash(before: &str, region: &str, after: &str) -> String {
    let mut buf = String::with_capacity(before.len() + region.len() + after.len() + 2);
    buf.push_str(before);
    buf.push('\u{1}');
    buf.push_str(region);
    buf.push('\u{1}');
    buf.push_str(after);
    region_hash(&buf)
}

pub struct Document {
    path: std::path::PathBuf,
    text: String,
    clock: u64,

    history: Vec<CrdtUpdate>,

    outbox: Vec<CrdtUpdate>,

    consumed_seq: u64,

    seen: std::collections::HashMap<String, u64>,

    needs_snapshot_publish: bool,

    disk_mtime: Option<std::time::SystemTime>,
    disk_len: u64,
    disk_hash: String,
}

impl Document {

    pub fn from_path(path: &Path) -> Result<Self, CrdtError> {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(CrdtError::Io(e)),
        };
        let seed = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let (disk_mtime, disk_len) = read_disk_stamp(path);
        let disk_hash = region_hash(&text);
        Ok(Self {
            path: path.to_path_buf(),
            text,
            clock: seed,
            history: Vec::new(),
            outbox: Vec::new(),
            consumed_seq: 0,
            seen: std::collections::HashMap::new(),
            needs_snapshot_publish: false,
            disk_mtime,
            disk_len,
            disk_hash,
        })
    }

    fn next_clock(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn note_seen_internal(&mut self, site: &str, clock: u64) {
        let entry = self.seen.entry(site.to_string()).or_insert(0);
        *entry = (*entry).max(clock);
    }

    pub fn note_seen(&mut self, site: &str, clock: u64) {
        self.note_seen_internal(site, clock);
        self.clock = self.clock.max(clock);
    }

    fn record_local(&mut self, update: CrdtUpdate) {
        self.note_seen_internal(update.site(), update.clock());
        self.history.push(update.clone());
        self.prune_history();
        coordination_metrics::incr_crdt_local_ops(1);
        if self.needs_snapshot_publish {
            return;
        }
        if self.outbox.len() >= MAX_OUTBOX_ENTRIES {
            self.needs_snapshot_publish = true;
            self.outbox.clear();
            return;
        }
        self.outbox.push(update);
    }

    pub fn apply_local(&mut self, op: &EditOp, site: &str) -> Result<(), CrdtError> {
        match op {
            EditOp::Replace {
                path,
                byte_range,
                new_text,
                ..
            } => {
                let start = byte_range.start;
                let end = byte_range.end;
                if end > self.text.len() {
                    return Err(CrdtError::UnsupportedOp(format!(
                        "replace range {start}..{end} exceeds doc len {}",
                        self.text.len()
                    )));
                }
                if start > end
                    || !self.text.is_char_boundary(start)
                    || !self.text.is_char_boundary(end)
                {
                    return Err(CrdtError::UnsupportedOp(format!(
                        "replace range {start}..{end} is not aligned to UTF-8 boundaries"
                    )));
                }
                let pre_hash = Some(replace_context_hash(
                    window_before(&self.text, start),
                    &self.text[start..end],
                    window_after(&self.text, end),
                ));
                self.text.replace_range(start..end, new_text);
                let clock = self.next_clock();
                self.record_local(CrdtUpdate::Replace {
                    clock,
                    site: site.to_string(),
                    path: path.display().to_string(),
                    start,
                    end,
                    text: new_text.clone(),
                    pre_hash,
                });
            }
            EditOp::Insert {
                path, at_byte, text, ..
            } => {
                let at = *at_byte;
                if at > self.text.len() {
                    return Err(CrdtError::UnsupportedOp(format!(
                        "insert at {at} exceeds doc len {}",
                        self.text.len()
                    )));
                }
                if !self.text.is_char_boundary(at) {
                    return Err(CrdtError::UnsupportedOp(format!(
                        "insert at {at} is not a UTF-8 character boundary"
                    )));
                }
                let pre_hash = Some(context_pair_hash(&self.text, at, at));
                self.text.insert_str(at, text);
                let clock = self.next_clock();
                self.record_local(CrdtUpdate::Insert {
                    clock,
                    site: site.to_string(),
                    path: path.display().to_string(),
                    at,
                    text: text.clone(),
                    pre_hash,
                });
            }
            EditOp::Delete {
                path, byte_range, ..
            } => {
                let start = byte_range.start;
                let end = byte_range.end;
                if end > self.text.len() {
                    return Err(CrdtError::UnsupportedOp(format!(
                        "delete range {start}..{end} exceeds doc len {}",
                        self.text.len()
                    )));
                }
                if start > end
                    || !self.text.is_char_boundary(start)
                    || !self.text.is_char_boundary(end)
                {
                    return Err(CrdtError::UnsupportedOp(format!(
                        "delete range {start}..{end} is not aligned to UTF-8 boundaries"
                    )));
                }
                let pre_hash = Some(context_pair_hash(&self.text, start, end));
                self.text.replace_range(start..end, "");
                let clock = self.next_clock();
                self.record_local(CrdtUpdate::Delete {
                    clock,
                    site: site.to_string(),
                    path: path.display().to_string(),
                    start,
                    end,
                    pre_hash,
                });
            }
            other => {
                return Err(CrdtError::UnsupportedOp(format!(
                    "{:?} is not representable in the CRDT POC",
                    other
                )));
            }
        }
        Ok(())
    }

    pub fn observe_local(&mut self, op: &EditOp, site: &str) -> Result<(), CrdtError> {
        self.resync_from_disk()?;
        match op {
            EditOp::Replace {
                path,
                byte_range,
                old_text,
                new_text,
                ..
            } => {
                let after_start = byte_range.start.saturating_add(new_text.len());
                let pre_hash = Some(replace_context_hash(
                    window_before(&self.text, byte_range.start),
                    old_text,
                    window_after(&self.text, after_start),
                ));
                let clock = self.next_clock();
                self.record_local(CrdtUpdate::Replace {
                    clock,
                    site: site.to_string(),
                    path: path.display().to_string(),
                    start: byte_range.start,
                    end: byte_range.end,
                    text: new_text.clone(),
                    pre_hash,
                });
            }
            EditOp::Insert {
                path, at_byte, text, ..
            } => {
                let after_start = at_byte.saturating_add(text.len());
                let pre_hash = Some(context_pair_hash(&self.text, *at_byte, after_start));
                let clock = self.next_clock();
                self.record_local(CrdtUpdate::Insert {
                    clock,
                    site: site.to_string(),
                    path: path.display().to_string(),
                    at: *at_byte,
                    text: text.clone(),
                    pre_hash,
                });
            }
            EditOp::Delete {
                path, byte_range, ..
            } => {
                let pre_hash = Some(context_pair_hash(
                    &self.text,
                    byte_range.start,
                    byte_range.start,
                ));
                let clock = self.next_clock();
                self.record_local(CrdtUpdate::Delete {
                    clock,
                    site: site.to_string(),
                    path: path.display().to_string(),
                    start: byte_range.start,
                    end: byte_range.end,
                    pre_hash,
                });
            }
            other => {
                return Err(CrdtError::UnsupportedOp(format!(
                    "{:?} is not representable in the CRDT store",
                    other
                )));
            }
        }
        Ok(())
    }

    pub fn resync_from_disk(&mut self) -> Result<(), CrdtError> {
        self.text = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(CrdtError::Io(e)),
        };
        let (mtime, len) = read_disk_stamp(&self.path);
        self.disk_mtime = mtime;
        self.disk_len = len;
        self.disk_hash = region_hash(&self.text);
        Ok(())
    }

    pub fn ensure_disk_fresh(&mut self) -> Result<bool, CrdtError> {
        let (mtime, len) = read_disk_stamp(&self.path);
        if mtime.is_some() && mtime == self.disk_mtime && len == self.disk_len {
            return Ok(false);
        }
        let disk_text = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(CrdtError::Io(e)),
        };
        let disk_hash = region_hash(&disk_text);
        if disk_hash == self.disk_hash {
            self.disk_mtime = mtime;
            self.disk_len = len;
            return Ok(false);
        }
        self.text = disk_text;
        self.disk_hash = disk_hash;
        self.disk_mtime = mtime;
        self.disk_len = len;
        Ok(true)
    }

    fn prune_history(&mut self) {
        if self.history.len() > MAX_HISTORY_ENTRIES {
            let drop = self.history.len() - MAX_HISTORY_ENTRIES;
            self.history.drain(..drop);
        }
    }

    pub fn apply_remote_update(
        &mut self,
        parsed: CrdtUpdate,
        local_site: &str,
    ) -> RemoteApplyOutcome {
        if !local_site.is_empty() && parsed.site() == local_site {
            self.clock = self.clock.max(parsed.clock());
            self.note_seen_internal(parsed.site(), parsed.clock());
            return RemoteApplyOutcome::OwnOp;
        }
        if self.seen.get(parsed.site()).copied().unwrap_or(0) >= parsed.clock() {
            return RemoteApplyOutcome::Duplicate;
        }

        let verdict: Result<(), &'static str> = match &parsed {
            CrdtUpdate::Replace {
                start,
                end,
                pre_hash,
                ..
            } => {
                if *start > *end
                    || *end > self.text.len()
                    || !self.text.is_char_boundary(*start)
                    || !self.text.is_char_boundary(*end)
                {
                    Err("replace range out of bounds for local text")
                } else {
                    match pre_hash {
                        Some(h)
                            if replace_context_hash(
                                window_before(&self.text, *start),
                                &self.text[*start..*end],
                                window_after(&self.text, *end),
                            ) == *h =>
                        {
                            Ok(())
                        }
                        Some(_) => Err("replace pre-image mismatch"),
                        None => Err("replace missing pre-image"),
                    }
                }
            }
            CrdtUpdate::Insert { at, pre_hash, .. } => {
                if *at > self.text.len() || !self.text.is_char_boundary(*at) {
                    Err("insert offset out of bounds for local text")
                } else {
                    match pre_hash {
                        Some(h) if context_pair_hash(&self.text, *at, *at) == *h => Ok(()),
                        Some(_) => Err("insert context mismatch"),
                        None => Err("insert missing pre-image"),
                    }
                }
            }
            CrdtUpdate::Delete {
                start,
                end,
                pre_hash,
                ..
            } => {
                if *start > *end
                    || *end > self.text.len()
                    || !self.text.is_char_boundary(*start)
                    || !self.text.is_char_boundary(*end)
                {
                    Err("delete range out of bounds for local text")
                } else {
                    match pre_hash {
                        Some(h) if context_pair_hash(&self.text, *start, *end) == *h => Ok(()),
                        Some(_) => Err("delete context mismatch"),
                        None => Err("delete missing pre-image"),
                    }
                }
            }
        };

        match verdict {
            Ok(()) => {
                match &parsed {
                    CrdtUpdate::Replace {
                        start, end, text, ..
                    } => self.text.replace_range(*start..*end, text),
                    CrdtUpdate::Insert { at, text, .. } => self.text.insert_str(*at, text),
                    CrdtUpdate::Delete { start, end, .. } => {
                        self.text.replace_range(*start..*end, "")
                    }
                }
                self.clock = self.clock.max(parsed.clock());
                self.note_seen_internal(parsed.site(), parsed.clock());
                self.history.push(parsed);
                self.prune_history();
                coordination_metrics::incr_crdt_remote_updates(1);
                RemoteApplyOutcome::Applied
            }
            Err(reason) => {
                coordination_metrics::incr_crdt_conflicts(1);
                RemoteApplyOutcome::Conflict(reason)
            }
        }
    }

    pub fn pending_updates(&self) -> Option<Vec<CrdtUpdate>> {
        if self.outbox.is_empty() {
            None
        } else {
            Some(self.outbox.clone())
        }
    }

    pub fn mark_exported(&mut self, count: usize) {
        let n = count.min(self.outbox.len());
        self.outbox.drain(..n);
    }

    pub fn consumed_seq(&self) -> u64 {
        self.consumed_seq
    }

    pub fn set_consumed_seq(&mut self, seq: u64) {
        self.consumed_seq = seq;
    }

    pub fn seen_clocks(&self) -> &std::collections::HashMap<String, u64> {
        &self.seen
    }

    pub fn restore_meta(&mut self, consumed_seq: u64, seen: std::collections::HashMap<String, u64>) {
        self.consumed_seq = self.consumed_seq.max(consumed_seq);
        for (site, clock) in seen {
            let entry = self.seen.entry(site).or_insert(0);
            *entry = (*entry).max(clock);
        }
        let max_seen = self.seen.values().copied().max().unwrap_or(0);
        self.clock = self.clock.max(max_seen);
    }

    pub fn has_pending(&self) -> bool {
        !self.outbox.is_empty()
    }

    pub fn needs_snapshot_publish(&self) -> bool {
        self.needs_snapshot_publish
    }

    pub fn mark_needs_snapshot_publish(&mut self) {
        self.needs_snapshot_publish = true;
        self.outbox.clear();
    }

    pub fn clear_needs_snapshot_publish(&mut self) {
        self.needs_snapshot_publish = false;
    }

    pub fn current_clock(&self) -> u64 {
        self.clock
    }

    pub fn current_text(&self) -> &str {
        &self.text
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::util::atomic_write(&self.path, self.text.as_bytes())?;
        let (mtime, len) = read_disk_stamp(&self.path);
        self.disk_mtime = mtime;
        self.disk_len = len;
        self.disk_hash = region_hash(&self.text);
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn read_disk_stamp(path: &Path) -> (Option<std::time::SystemTime>, u64) {
    match std::fs::metadata(path) {
        Ok(meta) => (meta.modified().ok(), meta.len()),
        Err(_) => (None, 0),
    }
}
