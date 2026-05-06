// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! ??CRDT proof-of-concept document.
//!
//! ## Why a stub?
//!
//! The plan calls out [`loro`] as the production CRDT crate.  In the
//! offline build environment we cannot fetch the dependency, so this
//! module currently ships a **minimal in-process stub** that mirrors
//! the public surface a real `loro::LoroDoc`-backed implementation
//! would expose:
//!
//! - [`Document::from_path`] ??load the file's current text (or
//!   start empty when the file does not exist).
//! - [`Document::apply_local`] ??translate the byte-range
//!   [`crate::apply_model::edit_op::EditOp`] variants
//!   (`Replace`/`Insert`/`Delete`) into in-memory text mutations and
//!   record an opaque "update" blob keyed by a monotonic clock.
//! - [`Document::apply_remote`] ??append a peer's update blob to the
//!   replay buffer (idempotent: duplicate updates are skipped).
//! - [`Document::encode_update`] ??emit every update accumulated
//!   since the last call as a single byte vector that callers can
//!   broadcast to peers (e.g. via Blackboard `crdt:<path>:<batch>`).
//! - [`Document::current_text`] ??current rendered text body.
//!
//! Swapping in real `loro` later only requires adding `dep:loro` to
//! the `crdt-coordination` feature, swapping the in-memory `text`
//! field for a `loro::LoroDoc`, and rewriting the four method
//! bodies.  Call sites in [`crate::apply_model::ops_applier`] do not
//! change.
//!
//! ## Wire-format
//!
//! Updates are encoded as JSON inside the byte vector for forward
//! compatibility ??a real Loro deployment would use the binary
//! `to_export_format` / `import_with` codec.  The JSON envelope is:
//!
//! ```json
//! {"clock": 12, "kind": "replace", "path": "src/foo.rs",
//!  "range": [0, 12], "text": "fn main() {}"}
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::apply_model::edit_op::EditOp;
use crate::observability::coordination_metrics;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CrdtUpdate {

    Replace {
        clock: u64,
        path: String,
        start: usize,
        end: usize,
        text: String,
    },

    Insert {
        clock: u64,
        path: String,
        at: usize,
        text: String,
    },

    Delete {
        clock: u64,
        path: String,
        start: usize,
        end: usize,
    },
}

impl CrdtUpdate {
    fn clock(&self) -> u64 {
        match self {
            CrdtUpdate::Replace { clock, .. }
            | CrdtUpdate::Insert { clock, .. }
            | CrdtUpdate::Delete { clock, .. } => *clock,
        }
    }
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

pub struct Document {
    path: std::path::PathBuf,
    text: String,
    clock: u64,

    history: Vec<CrdtUpdate>,

    last_export_clock: u64,
}

impl Document {

    pub fn from_path(path: &Path) -> Result<Self, CrdtError> {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(CrdtError::Io(e)),
        };
        Ok(Self {
            path: path.to_path_buf(),
            text,
            clock: 0,
            history: Vec::new(),
            last_export_clock: 0,
        })
    }

    fn next_clock(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    pub fn apply_local(&mut self, op: &EditOp) -> Result<(), CrdtError> {
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
                self.text.replace_range(start..end, new_text);
                let clock = self.next_clock();
                self.history.push(CrdtUpdate::Replace {
                    clock,
                    path: path.display().to_string(),
                    start,
                    end,
                    text: new_text.clone(),
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
                self.text.insert_str(at, text);
                let clock = self.next_clock();
                self.history.push(CrdtUpdate::Insert {
                    clock,
                    path: path.display().to_string(),
                    at,
                    text: text.clone(),
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
                self.text.replace_range(start..end, "");
                let clock = self.next_clock();
                self.history.push(CrdtUpdate::Delete {
                    clock,
                    path: path.display().to_string(),
                    start,
                    end,
                });
            }
            other => {
                return Err(CrdtError::UnsupportedOp(format!(
                    "{:?} is not representable in the CRDT POC",
                    other
                )));
            }
        }
        coordination_metrics::incr_crdt_local_ops(1);
        Ok(())
    }

    pub fn apply_remote(&mut self, update: &[u8]) -> Result<(), CrdtError> {
        let parsed: CrdtUpdate = serde_json::from_slice(update)
            .map_err(|e| CrdtError::Decode(format!("{e}")))?;
        let clock = parsed.clock();
        if self.history.iter().any(|u| u.clock() == clock) {
            return Ok(());
        }
        match &parsed {
            CrdtUpdate::Replace { start, end, text, .. } => {
                if *end <= self.text.len() {
                    self.text.replace_range(*start..*end, text);
                }
            }
            CrdtUpdate::Insert { at, text, .. } => {
                if *at <= self.text.len() {
                    self.text.insert_str(*at, text);
                }
            }
            CrdtUpdate::Delete { start, end, .. } => {
                if *end <= self.text.len() {
                    self.text.replace_range(*start..*end, "");
                }
            }
        }
        self.clock = self.clock.max(clock);
        self.history.push(parsed);
        coordination_metrics::incr_crdt_remote_updates(1);
        Ok(())
    }

    pub fn encode_update(&mut self) -> Vec<u8> {
        let new_updates: Vec<&CrdtUpdate> = self
            .history
            .iter()
            .filter(|u| u.clock() > self.last_export_clock)
            .collect();
        if new_updates.is_empty() {
            return Vec::new();
        }
        let last = new_updates
            .iter()
            .map(|u| u.clock())
            .max()
            .unwrap_or(self.last_export_clock);
        self.last_export_clock = last;
        serde_json::to_vec(&new_updates).unwrap_or_default()
    }

    pub fn current_text(&self) -> &str {
        &self.text
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, &self.text)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
