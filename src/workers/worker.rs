// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::Arc;

use arc_swap::ArcSwap;
use parking_lot::Mutex;
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

use crate::agent::TurnEvent;
use crate::workers::events::{WorkerMeta, WorkerResult, WorkerStatus, WorkerSummary};

#[derive(Debug, Clone)]
pub struct SequencedWorkerEvent {
    pub seq: u64,
    pub event: TurnEvent,
}

pub struct WorkerHandle {
    pub worker_id: String,
    pub parent_session_id: String,
    pub parent_tool_use_id: String,
    pub title: String,
    pub model: String,

    pub workspace_root: PathBuf,

    status: Arc<ArcSwap<WorkerStatus>>,

    last_action: Arc<Mutex<Option<String>>>,

    last_detail: Arc<Mutex<Option<String>>>,

    pub events_tx: broadcast::Sender<SequencedWorkerEvent>,

    pub cancel: CancellationToken,

    result: Arc<Mutex<Option<WorkerResult>>>,

    waiters: Arc<Mutex<Vec<oneshot::Sender<WorkerResult>>>>,

    pub started_at: chrono::DateTime<chrono::Utc>,

    pub finished_at: Arc<ArcSwap<Option<chrono::DateTime<chrono::Utc>>>>,

    resume_count: std::sync::atomic::AtomicU32,
}

impl WorkerHandle {
    pub fn new(
        worker_id: String,
        parent_session_id: String,
        parent_tool_use_id: String,
        title: String,
        model: String,
        workspace_root: PathBuf,
    ) -> Self {
        let (events_tx, _) = broadcast::channel::<SequencedWorkerEvent>(512);
        Self {
            worker_id,
            parent_session_id,
            parent_tool_use_id,
            title,
            model,
            workspace_root,
            status: Arc::new(ArcSwap::from_pointee(WorkerStatus::Pending)),
            last_action: Arc::new(Mutex::new(None)),
            last_detail: Arc::new(Mutex::new(None)),
            events_tx,
            cancel: CancellationToken::new(),
            result: Arc::new(Mutex::new(None)),
            waiters: Arc::new(Mutex::new(Vec::new())),
            started_at: chrono::Utc::now(),
            finished_at: Arc::new(ArcSwap::from_pointee(None)),
            resume_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn resume_count(&self) -> u32 {
        self.resume_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_resume_count(&self, count: u32) {
        self.resume_count
            .store(count, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn status(&self) -> WorkerStatus {
        **self.status.load()
    }

    pub fn set_status(&self, status: WorkerStatus) {
        self.status.store(Arc::new(status));
    }

    pub fn last_action(&self) -> Option<String> {
        self.last_action.lock().clone()
    }

    pub fn last_detail(&self) -> Option<String> {
        self.last_detail.lock().clone()
    }

    pub fn update_action(&self, action: Option<String>, detail: Option<String>) {
        if action.is_some() {
            *self.last_action.lock() = action;
        }
        if detail.is_some() {
            *self.last_detail.lock() = detail;
        }
    }

    pub fn finished_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        **self.finished_at.load()
    }

    pub fn mark_finished_now(&self) {
        self.finished_at.store(Arc::new(Some(chrono::Utc::now())));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SequencedWorkerEvent> {
        self.events_tx.subscribe()
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn wait(&self) -> oneshot::Receiver<WorkerResult> {
        let (tx, rx) = oneshot::channel();
        let result_guard = self.result.lock();
        match result_guard.clone() {
            Some(result) => {
                drop(result_guard);
                let _ = tx.send(result);
            }
            None => {
                self.waiters.lock().push(tx);
                drop(result_guard);
            }
        }
        rx
    }

    pub fn complete(&self, result: WorkerResult) {
        {
            let mut slot = self.result.lock();
            if slot.is_some() {
                return;
            }
            *slot = Some(result.clone());
        }
        let waiters: Vec<oneshot::Sender<WorkerResult>> = {
            let mut guard = self.waiters.lock();
            guard.drain(..).collect()
        };
        for w in waiters {
            let _ = w.send(result.clone());
        }
    }

    pub fn result_snapshot(&self) -> Option<WorkerResult> {
        self.result.lock().clone()
    }

    pub fn to_summary(&self) -> WorkerSummary {
        WorkerSummary {
            worker_id: self.worker_id.clone(),
            parent_session_id: self.parent_session_id.clone(),
            parent_tool_use_id: self.parent_tool_use_id.clone(),
            title: self.title.clone(),
            model: self.model.clone(),
            status: self.status(),
            last_action: self.last_action(),
            last_detail: self.last_detail(),
            started_at: self.started_at,
            finished_at: self.finished_at(),
        }
    }

    pub fn to_meta(
        &self,
        prompt: &str,
        context: Option<&str>,
        workspace_dir: Option<&str>,
    ) -> WorkerMeta {
        let result_snapshot = self.result_snapshot();
        WorkerMeta {
            worker_id: self.worker_id.clone(),
            parent_session_id: self.parent_session_id.clone(),
            parent_tool_use_id: self.parent_tool_use_id.clone(),
            title: self.title.clone(),
            prompt: prompt.to_string(),
            context: context.map(|s| s.to_string()),
            model: self.model.clone(),
            status: self.status(),
            last_action: self.last_action(),
            last_detail: self.last_detail(),
            started_at: self.started_at,
            finished_at: self.finished_at(),
            output: result_snapshot.as_ref().map(|r| r.output.clone()),
            error: result_snapshot.as_ref().and_then(|r| r.error.clone()),
            workspace_dir: workspace_dir.map(|s| s.to_string()),
            resume_count: self.resume_count(),
        }
    }
}
