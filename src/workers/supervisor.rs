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

const MAX_WORKERS_PER_PARENT: usize = 8;

fn max_global_workers() -> usize {
    crate::util::get_runtime_var("SEN_MAX_GLOBAL_WORKERS")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(32)
}

pub struct WorkerSupervisor {
    workers: DashMap<String, Arc<WorkerHandle>>,

    workspace_root: PathBuf,

    known_roots: dashmap::DashSet<PathBuf>,

    admission_lock: parking_lot::Mutex<()>,
}

impl WorkerSupervisor {
    pub fn new(workspace_root: PathBuf) -> Self {
        let known_roots = dashmap::DashSet::new();
        known_roots.insert(workspace_root.clone());
        Self {
            workers: DashMap::new(),
            workspace_root,
            known_roots,
            admission_lock: parking_lot::Mutex::new(()),
        }
    }

    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    pub fn known_roots(&self) -> Vec<PathBuf> {
        self.known_roots.iter().map(|r| r.key().clone()).collect()
    }

    pub fn note_known_root(&self, root: &std::path::Path) {
        self.known_roots.insert(root.to_path_buf());
    }

    pub fn register(&self, handle: Arc<WorkerHandle>) {
        self.known_roots.insert(handle.workspace_root.clone());
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

    pub fn active_count_total(&self) -> usize {
        self.workers
            .iter()
            .filter(|e| !e.value().status().is_terminal())
            .count()
    }

    pub fn admit_and_spawn_batch(
        self: &Arc<Self>,
        specs: Vec<WorkerSpec>,
        parent_draft_tx: Option<mpsc::Sender<DraftEvent>>,
        ctx: WorkerRunContext,
    ) -> Result<Vec<Arc<WorkerHandle>>, String> {
        if specs.is_empty() {
            return Ok(Vec::new());
        }
        let parent = specs[0].parent_session_id.clone();
        if parent.trim().is_empty() {
            return Err(
                "workers cannot be spawned without a parent session id: per-session quotas and \
                 cancel-on-parent-exit would silently break"
                    .to_string(),
            );
        }
        let _guard = self.admission_lock.lock();

        let per_parent = self.active_count_for_parent(&parent);
        if per_parent + specs.len() > MAX_WORKERS_PER_PARENT {
            return Err(format!(
                "worker quota exceeded: {per_parent} workers already active for this session and \
                 {} more requested (per-session limit {MAX_WORKERS_PER_PARENT}); wait for running \
                 workers to finish or cancel them first",
                specs.len()
            ));
        }
        let global = self.active_count_total();
        let global_cap = max_global_workers();
        if global + specs.len() > global_cap {
            return Err(format!(
                "global worker quota exceeded: {global} workers active process-wide and {} more \
                 requested (global limit {global_cap}, override with SEN_MAX_GLOBAL_WORKERS)",
                specs.len()
            ));
        }

        let mut handles = Vec::with_capacity(specs.len());
        for spec in specs {
            handles.push(self.spawn(spec, parent_draft_tx.clone(), ctx.clone()));
        }
        Ok(handles)
    }

    pub fn spawn(
        self: &Arc<Self>,
        spec: WorkerSpec,
        parent_draft_tx: Option<mpsc::Sender<DraftEvent>>,
        ctx: WorkerRunContext,
    ) -> Arc<WorkerHandle> {
        self.spawn_with_id(generate_worker_id(), 0, spec, parent_draft_tx, ctx)
    }

    pub fn spawn_resumed(
        self: &Arc<Self>,
        worker_id: String,
        resume_count: u32,
        spec: WorkerSpec,
        ctx: WorkerRunContext,
    ) -> Arc<WorkerHandle> {
        let _guard = self.admission_lock.lock();
        self.spawn_with_id(worker_id, resume_count, spec, None, ctx)
    }

    fn spawn_with_id(
        self: &Arc<Self>,
        worker_id: String,
        resume_count: u32,
        spec: WorkerSpec,
        parent_draft_tx: Option<mpsc::Sender<DraftEvent>>,
        ctx: WorkerRunContext,
    ) -> Arc<WorkerHandle> {
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
        handle.set_resume_count(resume_count);

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
    scan_and_recover_with_resume(workspace_root, None);
}

const MAX_WORKER_RESUME_ATTEMPTS: u32 = 2;

const WORKER_RESUME_BANNER: &str = "[resumed] This task was interrupted by a host restart. \
The workspace may already contain partial changes from the previous attempt; inspect the \
current state first, then finish the task idempotently.";

fn resume_prompt(original: &str) -> String {
    if original.starts_with("[resumed]") {
        original.to_string()
    } else {
        format!("{WORKER_RESUME_BANNER}\n\n{original}")
    }
}

pub fn scan_and_recover_with_resume(
    workspace_root: &std::path::Path,
    resume_ctx: Option<crate::workers::runner::WorkerRunContext>,
) {
    let interrupted =
        match crate::workers::persistence::scan_interrupted(workspace_root) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "WorkerSupervisor: startup worker scan failed"
                );
                Vec::new()
            }
        };
    let has_runtime = tokio::runtime::Handle::try_current().is_ok();
    let supervisor = global_supervisor();
    let mut failed = 0_usize;
    let mut resumed = 0_usize;
    for mut meta in interrupted {
        let over_limit = meta.resume_count >= MAX_WORKER_RESUME_ATTEMPTS;
        let worktree_gone = meta
            .workspace_dir
            .as_deref()
            .map(|d| !std::path::Path::new(d).is_dir())
            .unwrap_or(false);
        let quota_full = supervisor
            .as_ref()
            .map(|s| s.active_count_total() >= max_global_workers())
            .unwrap_or(true);
        let resumable = resume_ctx.is_some()
            && has_runtime
            && supervisor.is_some()
            && !meta.prompt.trim().is_empty()
            && !over_limit
            && !worktree_gone
            && !quota_full;
        if !resumable {
            let reason = if over_limit {
                "Worker was interrupted by repeated host restarts; giving up after the resume limit."
            } else if worktree_gone {
                "Worker was interrupted by a host restart and its worktree no longer exists; marked as failed."
            } else {
                "Worker did not finish before the host process restarted; marked as failed."
            };
            if let Err(err) = crate::workers::persistence::mark_worker_failed(
                workspace_root,
                &mut meta,
                reason,
            ) {
                tracing::warn!(
                    worker_id = %meta.worker_id,
                    error = %err,
                    "failed to persist recovered worker meta"
                );
            } else {
                failed += 1;
            }
            continue;
        }
        let (Some(sup), Some(ctx)) = (supervisor.as_ref(), resume_ctx.clone()) else {
            continue;
        };
        meta.resume_count += 1;
        meta.status = crate::workers::events::WorkerStatus::Pending;
        meta.error = None;
        meta.finished_at = None;
        if let Err(err) = crate::workers::persistence::write_meta(workspace_root, &meta) {
            tracing::warn!(
                worker_id = %meta.worker_id,
                error = %err,
                "failed to persist resumed worker meta"
            );
        }
        let spec = WorkerSpec {
            parent_session_id: meta.parent_session_id.clone(),
            parent_tool_use_id: meta.parent_tool_use_id.clone(),
            title: meta.title.clone(),
            prompt: resume_prompt(&meta.prompt),
            context: meta.context.clone(),
            model: Some(meta.model.clone()),
            workspace_dir: meta.workspace_dir.clone(),
        };
        sup.spawn_resumed(meta.worker_id.clone(), meta.resume_count, spec, ctx);
        resumed += 1;
    }
    if failed > 0 || resumed > 0 {
        tracing::info!(
            failed,
            resumed,
            "WorkerSupervisor: startup recovery finished (resumed interrupted workers where possible)"
        );
    }
    let root = workspace_root.to_path_buf();
    if has_runtime {
        crate::runtime::spawn_supervised("workers.worktree_gc", async move {
            gc_stale_worker_worktrees(&root).await;
        });
    }
}

pub fn candidate_worker_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(sup) = global_supervisor() {
        out.extend(sup.known_roots());
    }
    if let Some(state) = crate::bootstrap::try_get_state() {
        let cwd = state.read(|s| s.cwd.clone());
        if !cwd.as_os_str().is_empty() {
            out.push(cwd);
        }
    }
    if out.is_empty() {
        out.push(PathBuf::from("."));
    }
    out.sort();
    out.dedup();
    out
}

const WORKTREE_GC_MIN_AGE_SECS: u64 = 3 * 60 * 60;

pub async fn gc_stale_worker_worktrees(workspace_root: &std::path::Path) {
    let dir = workspace_root.join(".sen").join("worktrees");
    let Ok(mut rd) = tokio::fs::read_dir(&dir).await else {
        return;
    };
    let protected: std::collections::HashSet<PathBuf> = {
        let root = workspace_root.to_path_buf();
        tokio::task::spawn_blocking(move || {
            crate::workers::persistence::list_meta(&root)
                .unwrap_or_default()
                .into_iter()
                .filter(|m| !m.status.is_terminal())
                .filter_map(|m| m.workspace_dir.map(PathBuf::from))
                .map(|p| p.canonicalize().unwrap_or(p))
                .collect()
        })
        .await
        .unwrap_or_default()
    };
    let root_str = workspace_root.to_string_lossy().to_string();
    let mut reclaimed = 0usize;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if protected.contains(&canonical) || protected.contains(&path) {
            continue;
        }
        if !worktree_is_stale(&path).await {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        let _ = crate::util::hidden_async_command("git")
            .args(["-C", &path_str, "add", "-A"])
            .output()
            .await;
        let staged = crate::util::hidden_async_command("git")
            .args(["-C", &path_str, "diff", "--cached", "--quiet"])
            .output()
            .await;
        if matches!(staged, Ok(ref o) if !o.status.success()) {
            let _ = crate::util::hidden_async_command("git")
                .args([
                    "-C",
                    &path_str,
                    "commit",
                    "-m",
                    "sen-worker: salvaged from stale worktree during startup GC",
                    "--no-verify",
                ])
                .output()
                .await;
        }
        let removed = crate::util::hidden_async_command("git")
            .args(["-C", &root_str, "worktree", "remove", "--force", &path_str])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !removed {
            let _ = tokio::fs::remove_dir_all(&path).await;
        }
        reclaimed += 1;
    }
    if reclaimed > 0 {
        let _ = crate::util::hidden_async_command("git")
            .args(["-C", &root_str, "worktree", "prune"])
            .output()
            .await;
        tracing::info!(
            reclaimed,
            "worker worktree GC reclaimed stale worktree directories (branches kept)"
        );
    }
}

async fn worktree_is_stale(path: &std::path::Path) -> bool {
    let meta_time = tokio::fs::metadata(path.join(".git"))
        .await
        .ok()
        .and_then(|m| m.modified().ok().or_else(|| m.created().ok()));
    let Some(t) = meta_time else {
        return true;
    };
    match t.elapsed() {
        Ok(age) => age.as_secs() >= WORKTREE_GC_MIN_AGE_SECS,
        Err(_) => false,
    }
}
