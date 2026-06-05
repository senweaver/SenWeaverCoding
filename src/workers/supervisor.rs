// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use tokio::sync::mpsc;

use crate::agent::loop_::DraftEvent;
use crate::workers::events::{WorkerSpec, WorkerSummary};
use crate::workers::runner::WorkerRunContext;
use crate::workers::worker::WorkerHandle;

const WORKER_ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn generate_worker_id() -> String {
    let mut suffix = String::with_capacity(9);
    suffix.push('w');
    for _ in 0..8 {
        let idx = rand::random_range(0..WORKER_ID_ALPHABET.len());
        suffix.push(WORKER_ID_ALPHABET[idx] as char);
    }
    suffix
}

pub struct WorkerSupervisor {
    workers: DashMap<String, Arc<WorkerHandle>>,

    workspace_root: PathBuf,
}

impl WorkerSupervisor {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workers: DashMap::new(),
            workspace_root,
        }
    }

    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    pub fn register(&self, handle: Arc<WorkerHandle>) {
        self.workers.insert(handle.worker_id.clone(), handle);
    }

    pub fn unregister(&self, worker_id: &str) {
        self.workers.remove(worker_id);
    }

    pub fn get(&self, worker_id: &str) -> Option<Arc<WorkerHandle>> {
        self.workers.get(worker_id).map(|r| Arc::clone(r.value()))
    }

    pub fn cancel(&self, worker_id: &str) -> bool {
        if let Some(h) = self.get(worker_id) {
            h.cancel();
            true
        } else {
            false
        }
    }

    pub fn cancel_for_parent(&self, parent_session_id: &str) -> usize {
        let mut count = 0_usize;
        for entry in self.workers.iter() {
            let h = entry.value();
            if h.parent_session_id == parent_session_id && !h.status().is_terminal() {
                h.cancel();
                count += 1;
            }
        }
        count
    }

    pub fn list_by_parent(&self, parent_session_id: &str) -> Vec<WorkerSummary> {
        let mut out: Vec<WorkerSummary> = self
            .workers
            .iter()
            .filter(|e| e.value().parent_session_id == parent_session_id)
            .map(|e| e.value().to_summary())
            .collect();
        out.sort_by_key(|s| s.started_at);
        out
    }

    pub fn summary_for(&self, worker_id: &str) -> Option<WorkerSummary> {
        self.get(worker_id).map(|h| h.to_summary())
    }

    pub fn all_summaries(&self) -> Vec<WorkerSummary> {
        let mut out: Vec<WorkerSummary> = self
            .workers
            .iter()
            .map(|e| e.value().to_summary())
            .collect();
        out.sort_by_key(|s| s.started_at);
        out
    }

    pub fn active_count_for_parent(&self, parent_session_id: &str) -> usize {
        self.workers
            .iter()
            .filter(|e| {
                let h = e.value();
                h.parent_session_id == parent_session_id && !h.status().is_terminal()
            })
            .count()
    }

    pub fn spawn(
        self: &Arc<Self>,
        spec: WorkerSpec,
        parent_draft_tx: Option<mpsc::Sender<DraftEvent>>,
        ctx: WorkerRunContext,
    ) -> Arc<WorkerHandle> {
        let worker_id = generate_worker_id();
        let model = spec
            .model
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                ctx.config
                    .default_model
                    .clone()
                    .unwrap_or_else(|| "default".to_string())
            });
        let effective_workspace_root = ctx
            .parent_workspace_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace_root.clone());
        let handle = Arc::new(WorkerHandle::new(
            worker_id.clone(),
            spec.parent_session_id.clone(),
            spec.parent_tool_use_id.clone(),
            spec.title.clone(),
            model,
            effective_workspace_root,
        ));

        self.register(Arc::clone(&handle));

        let supervisor = Arc::clone(self);
        let handle_for_task = Arc::clone(&handle);
        let spec_for_task = spec.clone();
        let ctx_for_task = ctx.clone();

        crate::runtime::spawn_supervised("worker.runner", async move {
            crate::workers::runner::run_worker(
                supervisor,
                handle_for_task,
                spec_for_task,
                parent_draft_tx,
                ctx_for_task,
            )
            .await;
        });

        handle
    }
}

static SUPERVISOR: OnceLock<Arc<WorkerSupervisor>> = OnceLock::new();

pub fn init_global_supervisor(workspace_root: PathBuf) -> Arc<WorkerSupervisor> {
    let sup = SUPERVISOR.get_or_init(|| Arc::new(WorkerSupervisor::new(workspace_root)));
    Arc::clone(sup)
}

pub fn global_supervisor() -> Option<Arc<WorkerSupervisor>> {
    SUPERVISOR.get().cloned()
}

pub fn try_init_default() -> Option<Arc<WorkerSupervisor>> {
    let root = if let Some(state) = crate::bootstrap::try_get_state() {
        let cwd = state.read(|s| s.cwd.clone());
        if cwd.as_os_str().is_empty() {
            return None;
        }
        cwd
    } else {
        std::env::current_dir().ok()?
    };
    Some(init_global_supervisor(root))
}

pub fn ensure_supervisor() -> anyhow::Result<Arc<WorkerSupervisor>> {
    if let Some(s) = global_supervisor() {
        return Ok(s);
    }
    try_init_default()
        .ok_or_else(|| anyhow::anyhow!("worker supervisor not initialised and no cwd available"))
}

pub fn scan_and_recover_at(workspace_root: &std::path::Path) {
    match crate::workers::persistence::scan_and_recover(workspace_root) {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            recovered = n,
            "WorkerSupervisor: marked {n} stale workers as failed during startup recovery"
        ),
        Err(err) => tracing::warn!(
            error = %err,
            "WorkerSupervisor: scan_and_recover failed"
        ),
    }
}
