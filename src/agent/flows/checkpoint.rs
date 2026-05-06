// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Lightweight in-memory checkpoint store used by [`super::Flow`]
//! implementations that want to support rollback.  The full
//! `flow_rollback` tool integration lands in D5.4.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::checkpoint_backend::{CheckpointBackend, CheckpointMeta};
use super::traits::{Artifact, TranscriptEntry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub label: String,
    pub artifacts: Vec<Artifact>,

    #[serde(skip)]
    pub transcript: Vec<TranscriptEntry>,

    #[serde(default)]
    pub edit_batch_id: Option<String>,

    #[serde(default)]
    pub session_id: Option<String>,
}

impl Checkpoint {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        artifacts: Vec<Artifact>,
        transcript: Vec<TranscriptEntry>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            artifacts,
            transcript,
            edit_batch_id: None,
            session_id: None,
        }
    }

    #[must_use]
    pub fn with_edit_batch_id(mut self, id: impl Into<String>) -> Self {
        self.edit_batch_id = Some(id.into());
        self
    }

    #[must_use]
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }
}

pub struct CheckpointStore {
    inner: RwLock<VecDeque<Checkpoint>>,
    capacity: usize,
    backend: Option<Arc<dyn CheckpointBackend>>,
}

impl CheckpointStore {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            inner: RwLock::new(VecDeque::with_capacity(cap)),
            capacity: cap,
            backend: None,
        }
    }

    #[must_use]
    pub fn with_backend(mut self, backend: Arc<dyn CheckpointBackend>) -> Self {
        self.backend = Some(backend);
        self
    }

    pub fn has_backend(&self) -> bool {
        self.backend.is_some()
    }

    pub fn push(&self, cp: Checkpoint) {
        {
            let mut guard = self.inner.write();
            if guard.len() >= self.capacity {
                guard.pop_front();
            }
            guard.push_back(cp.clone());
        }

        if let Some(backend) = self.backend.clone() {
            let Some(session_id) = cp.session_id.clone() else {
                tracing::debug!(
                    cp_id = %cp.id,
                    "checkpoint has no session_id; persistent save skipped"
                );
                return;
            };
            let cp_owned = cp;
            tokio::spawn(async move {
                match backend.save(&session_id, &cp_owned).await {
                    Ok(()) => crate::observability::session_write_mode_metrics::incr_checkpoint_persisted(),
                    Err(err) => {
                        crate::observability::session_write_mode_metrics::incr_checkpoint_backend_error();
                        tracing::warn!(error = %err, "persistent checkpoint save failed");
                    }
                }
            });
        }
    }

    pub async fn list_persisted(&self, session_id: &str) -> Vec<CheckpointMeta> {
        if let Some(backend) = self.backend.clone() {
            match backend.list(session_id).await {
                Ok(metas) => return metas,
                Err(err) => {
                    crate::observability::session_write_mode_metrics::incr_checkpoint_backend_error();
                    tracing::warn!(error = %err, "persistent checkpoint list failed");
                }
            }
        }
        Vec::new()
    }

    pub async fn load_persisted(
        &self,
        session_id: &str,
        checkpoint_id: &str,
    ) -> Option<Checkpoint> {
        let backend = self.backend.clone()?;
        match backend.load(session_id, checkpoint_id).await {
            Ok(cp) => Some(cp),
            Err(err) => {
                crate::observability::session_write_mode_metrics::incr_checkpoint_backend_error();
                tracing::warn!(error = %err, "persistent checkpoint load failed");
                None
            }
        }
    }

    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    pub fn peek(&self) -> Option<Checkpoint> {
        self.inner.read().back().cloned()
    }

    pub fn rollback(&self, n: usize) -> Option<Checkpoint> {
        if n == 0 {
            return self.peek();
        }
        let mut guard = self.inner.write();
        if guard.len() <= n {
            return None;
        }
        for _ in 0..n {
            guard.pop_back();
        }
        guard.back().cloned()
    }

    pub fn snapshot(&self) -> Vec<Checkpoint> {
        self.inner.read().iter().cloned().collect()
    }

    #[doc(hidden)]
    pub fn clear(&self) {
        self.inner.write().clear();
    }
}

impl Default for CheckpointStore {
    fn default() -> Self {
        Self::new(32)
    }
}
