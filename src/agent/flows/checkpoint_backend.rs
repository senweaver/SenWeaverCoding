// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! pluggable persistence for flow checkpoints.
//!
//! The legacy [`super::CheckpointStore`] kept every
//! [`super::Checkpoint`] in an in-process FIFO, which was fine for
//! the single-command rollback window (`flow_rollback steps=N`)
//! but has no way to recover state after the agent process
//! exits.  This module introduces a trait-driven extension:
//!
//! * [`CheckpointBackend`] defines the async surface a backend
//!   has to implement (save / load / list).
//! * [`PersistentCheckpointBackend`] is the first concrete
//!   implementation and writes every checkpoint as a standalone
//!   JSON file under `./.sen/checkpoints/<session_id>/<cp_id>.json`.
//!   Listing a session is a single directory scan, so a future
//!   cross-process `flow_rollback --session --checkpoint` can find
//!   snapshots written by an earlier process.
//!
//! Failure policy: every backend error is logged at `warn` and
//! counted via
//! [`crate::observability::session_write_mode_metrics::incr_checkpoint_backend_error`];
//! the in-memory store stays authoritative during runtime so a
//! broken disk never stalls a flow.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::checkpoint::Checkpoint;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub session_id: String,
    pub checkpoint_id: String,
    pub label: String,
    pub edit_batch_id: Option<String>,
    pub bytes: u64,
}

#[async_trait]
pub trait CheckpointBackend: Send + Sync {

    async fn save(&self, session_id: &str, cp: &Checkpoint) -> Result<(), CheckpointBackendError>;

    async fn load(
        &self,
        session_id: &str,
        cp_id: &str,
    ) -> Result<Checkpoint, CheckpointBackendError>;

    async fn list(&self, session_id: &str)
    -> Result<Vec<CheckpointMeta>, CheckpointBackendError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CheckpointBackendError {
    #[error("checkpoint not found: {session_id}/{checkpoint_id}")]
    NotFound {
        session_id: String,
        checkpoint_id: String,
    },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub struct PersistentCheckpointBackend {
    root: PathBuf,
}

impl PersistentCheckpointBackend {

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn new_shared(root: impl Into<PathBuf>) -> Arc<dyn CheckpointBackend> {
        Arc::new(Self::new(root))
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join("checkpoints").join(sanitize(session_id))
    }

    fn file_path(&self, session_id: &str, cp_id: &str) -> PathBuf {
        self.session_dir(session_id)
            .join(format!("{}.json", sanitize(cp_id)))
    }
}

#[async_trait]
impl CheckpointBackend for PersistentCheckpointBackend {
    async fn save(&self, session_id: &str, cp: &Checkpoint) -> Result<(), CheckpointBackendError> {
        let dir = self.session_dir(session_id);
        fs::create_dir_all(&dir).await?;
        let path = self.file_path(session_id, &cp.id);
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(cp)?;
        {
            let mut file = fs::File::create(&tmp).await?;
            file.write_all(&bytes).await?;
            file.flush().await?;
        }
        fs::rename(&tmp, &path).await?;
        Ok(())
    }

    async fn load(
        &self,
        session_id: &str,
        cp_id: &str,
    ) -> Result<Checkpoint, CheckpointBackendError> {
        let path = self.file_path(session_id, cp_id);
        let bytes = match fs::read(&path).await {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(CheckpointBackendError::NotFound {
                    session_id: session_id.to_string(),
                    checkpoint_id: cp_id.to_string(),
                });
            }
            Err(err) => return Err(err.into()),
        };
        let cp: Checkpoint = serde_json::from_slice(&bytes)?;
        Ok(cp)
    }

    async fn list(
        &self,
        session_id: &str,
    ) -> Result<Vec<CheckpointMeta>, CheckpointBackendError> {
        let dir = self.session_dir(session_id);
        if !dir_exists(&dir).await {
            return Ok(Vec::new());
        }
        let mut entries = Vec::<(std::time::SystemTime, CheckpointMeta)>::new();
        let mut reader = fs::read_dir(&dir).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let meta = entry.metadata().await?;
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            match fs::read(&path).await {
                Ok(bytes) => match serde_json::from_slice::<Checkpoint>(&bytes) {
                    Ok(cp) => entries.push((
                        mtime,
                        CheckpointMeta {
                            session_id: session_id.to_string(),
                            checkpoint_id: cp.id,
                            label: cp.label,
                            edit_batch_id: cp.edit_batch_id,
                            bytes: meta.len(),
                        },
                    )),
                    Err(err) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %err,
                            "ignoring malformed checkpoint JSON"
                        );
                    }
                },
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "checkpoint read failed");
                }
            }
        }
        entries.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));
        Ok(entries.into_iter().map(|(_, m)| m).collect())
    }
}

async fn dir_exists(path: &Path) -> bool {
    match fs::metadata(path).await {
        Ok(m) => m.is_dir(),
        Err(_) => false,
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '_',
        })
        .collect()
}
