// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::apply_model::edit_op::EditOp;
use crate::observability::coordination_metrics;

const MAX_HISTORY_ENTRIES: usize = 10_000;
const MAX_OUTBOX_ENTRIES: usize = 4_096;
const CONTEXT_WINDOW_BYTES: usize = 64;

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

pub fn context_pair_hash(text: &str, before_end: usize, after_start: usize) -> String {
    let be = snap_left(text, before_end);
    let bs = snap_left(text, be.saturating_sub(CONTEXT_WINDOW_BYTES));
    let a_start = snap_right(text, after_start.min(text.len()));
    let a_end = snap_right(
        text,
        after_start
            .saturating_add(CONTEXT_WINDOW_BYTES)
            .min(text.len()),
    );
    let mut buf = String::with_capacity((be - bs) + (a_end - a_start) + 1);
    buf.push_str(&text[bs..be]);
    buf.push('\u{1}');
    if a_start < a_end {
        buf.push_str(&text[a_start..a_end]);
    }
    region_hash(&buf)
}

pub struct Document {
    path: std::path::PathBuf,
    text: String,
    clock: u64,

    history: Vec<CrdtUpdate>,

    outbox: Vec<CrdtUpdate>,

    consumed_seq: u64,
}

impl Document {

    pub fn from_path(path: &Path) -> Result<Self, CrdtError> {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(CrdtError::Io(e)),
        };
        let seed = chrono::Utc::now().timestamp_millis().max(0) as u64;
        Ok(Self {
            path: path.to_path_buf(),
            text,
            clock: seed,
            history: Vec::new(),
            outbox: Vec::new(),
            consumed_seq: 0,
        })
    }

    fn next_clock(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn record_local(&mut self, update: CrdtUpdate) {
        self.history.push(update.clone());
        if self.outbox.len() >= MAX_OUTBOX_ENTRIES {
            let drop = self.outbox.len() + 1 - MAX_OUTBOX_ENTRIES;
            self.outbox.drain(..drop);
        }
        self.outbox.push(update);
        self.prune_history();
        coordination_metrics::incr_crdt_local_ops(1);
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
                let pre_hash = Some(region_hash(&self.text[start..end]));
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
                let clock = self.next_clock();
                self.record_local(CrdtUpdate::Replace {
                    clock,
                    site: site.to_string(),
                    path: path.display().to_string(),
                    start: byte_range.start,
                    end: byte_range.end,
                    text: new_text.clone(),
                    pre_hash: Some(region_hash(old_text)),
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
        Ok(())
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
            return RemoteApplyOutcome::OwnOp;
        }
        if self
            .history
            .iter()
            .any(|u| u.site() == parsed.site() && u.clock() == parsed.clock())
        {
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
                        Some(h) if region_hash(&self.text[*start..*end]) == *h => Ok(()),
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

    pub fn current_text(&self) -> &str {
        &self.text
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::util::atomic_write(&self.path, self.text.as_bytes())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
