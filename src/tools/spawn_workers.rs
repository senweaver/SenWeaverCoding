// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::traits::{Tool, ToolResult};
use crate::agent::loop_::{DraftEvent, current_tool_call_id, take_parent_draft_channel};
use crate::config::Config;
use crate::config::live::LiveConfig;
use crate::session::current_session_context;
use crate::workers::events::{WorkerResult, WorkerSpec, WorkerStatus};
use crate::workers::runner::WorkerRunContext;
use crate::workers::supervisor::ensure_supervisor;
use crate::workers::worker::WorkerHandle;
use crate::workers::worktree::{
    WorktreeInfo, commit_and_merge_worker, create_worker_worktree, parent_workspace_is_dirty,
    remove_worker_worktree, salvage_worktree, worktree_change_report,
};

const TOOL_NAME: &str = "spawn_workers";

const DEFAULT_WORKERS_TIMEOUT_SECS: u64 = 1800;

const MAX_WORKERS_PER_CALL: usize = crate::constants::system::MAX_CONCURRENT_SUBAGENTS as usize;

const SUMMARY_INPUT_CAP_CHARS: usize = 6_000;
const JUDGE_INPUT_CAP_CHARS: usize = 8_000;

#[derive(Debug, Deserialize)]
struct SpawnArgs {
    #[serde(default)]
    tasks: Vec<TaskArgs>,

    #[serde(default = "default_merge_strategy")]
    merge_strategy: MergeStrategy,

    #[serde(default)]
    workers_timeout_secs: Option<u64>,

    #[serde(default = "default_isolation")]
    isolation: Isolation,

    #[serde(default = "default_auto_merge")]
    auto_merge: bool,

    #[serde(default)]
    allow_shared_fallback: bool,
}

#[derive(Debug, Deserialize)]
struct TaskArgs {
    #[serde(default)]
    title: Option<String>,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MergeStrategy {
    Concat,
    Summary,
    BestOfN,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Isolation {
    Shared,
    Worktree,
}

#[derive(Clone, Copy)]
enum MergePrompt {
    Summary,
    Judge,
}

fn default_merge_strategy() -> MergeStrategy {
    MergeStrategy::Concat
}

fn default_isolation() -> Isolation {
    Isolation::Worktree
}

fn default_auto_merge() -> bool {
    true
}

const WORKER_PORT_BASE: u16 = 4100;
const WORKER_PORT_RANGE: usize = 512;

static WORKER_PORT_CURSOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn allocate_worker_port() -> u16 {
    let off = WORKER_PORT_CURSOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        % WORKER_PORT_RANGE;
    WORKER_PORT_BASE.saturating_add(off as u16)
}

struct WorkerBatchCancelGuard {
    handles: Vec<Arc<WorkerHandle>>,
    worktrees: Vec<Option<WorktreeInfo>>,
    disarmed: bool,
}

impl WorkerBatchCancelGuard {
    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for WorkerBatchCancelGuard {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        let mut cancelled = 0usize;
        for h in &self.handles {
            if h.result_snapshot().is_none() {
                h.cancel();
                cancelled += 1;
            }
        }
        if cancelled > 0 {
            tracing::warn!(
                cancelled,
                "spawn_workers tool future dropped before completion; \
                 propagated cancellation to still-running workers"
            );
        }
        let handles = std::mem::take(&mut self.handles);
        let worktrees: Vec<WorktreeInfo> =
            std::mem::take(&mut self.worktrees).into_iter().flatten().collect();
        if worktrees.is_empty() {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Err(_) => {
                for info in &worktrees {
                    tracing::error!(
                        branch = %info.branch,
                        path = %info.path.display(),
                        "no tokio runtime available while dropping worker batch; worktree \
                         salvage abandoned — uncommitted work may remain in this directory"
                    );
                }
            }
            Ok(rt) => {
                rt.spawn(async move {
                    let grace_deadline =
                        tokio::time::Instant::now() + std::time::Duration::from_secs(30);
                    let mut grace_waits = tokio::task::JoinSet::new();
                    for h in &handles {
                        let wait = h.wait();
                        grace_waits.spawn(async move {
                            let _ = tokio::time::timeout_at(grace_deadline, wait).await;
                        });
                    }
                    while grace_waits.join_next().await.is_some() {}
                    let base_lock = worktrees
                        .first()
                        .map(|info| crate::workers::worktree::base_merge_lock(&info.base));
                    let _base_guard = match base_lock.as_ref() {
                        Some(lock) => Some(lock.lock().await),
                        None => None,
                    };
                    for info in &worktrees {
                        let salvage = salvage_worktree(info).await;
                        if salvage.retained {
                            tracing::warn!(
                                branch = %info.branch,
                                path = %info.path.display(),
                                "cancelled worker worktree salvage: {}",
                                salvage.note
                            );
                        } else {
                            tracing::info!(
                                branch = %info.branch,
                                "cancelled worker worktree salvage: {}",
                                salvage.note
                            );
                        }
                    }
                });
            }
        }
    }
}

pub struct SpawnWorkersTool {
    config: Arc<Config>,
    live_config: Option<LiveConfig>,
}

impl SpawnWorkersTool {
    pub fn new(config: Arc<Config>, live_config: Option<LiveConfig>) -> Self {
        Self { config, live_config }
    }
}

#[async_trait]
impl Tool for SpawnWorkersTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        "Run multiple long-running subtasks as parallel worker sub-agents. Each worker has its own \
         session and detail tab in the UI. Use when you need to decompose a complex request into \
         independent tasks that can run concurrently (e.g. analysing multiple codebases, fanning \
         out research, executing several long jobs). Default isolation is \"worktree\" so each \
         worker gets its own git worktree + branch and a unique DEV port; set \
         isolation=\"shared\" only for read-only/research fan-out. When isolation is worktree, \
         successful workers are merged sequentially into the parent workspace (auto_merge=true \
         by default) with conflict abort + report; if the parent workspace has uncommitted \
         changes the auto-merge is skipped and the branches are preserved for manual review. \
         Set merge_strategy=\"best_of_n\" to run competing attempts: a judge picks the single \
         best result and ONLY the winning branch is merged (losing branches are preserved, \
         their worktrees removed). The tool blocks until every worker reaches a terminal state \
         and returns an aggregated result. Workers cannot spawn additional workers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "tasks": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_WORKERS_PER_CALL,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["prompt"],
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Short human-readable label shown in the worker UI."
                            },
                            "prompt": {
                                "type": "string",
                                "minLength": 1,
                                "description": "Full natural-language task for this worker."
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional model override; when omitted, falls back to the configured subagent model (agent_runtime.subagent_model) and then to the parent session's model."
                            },
                            "context": {
                                "type": "string",
                                "description": "Optional context prepended to the worker's prompt."
                            }
                        }
                    }
                },
                "merge_strategy": {
                    "type": "string",
                    "enum": ["concat", "summary", "best_of_n"],
                    "default": "concat",
                    "description": "How to aggregate worker outputs back to the parent: 'concat' joins full outputs; 'summary' produces an LLM-written synthesis of all workers; 'best_of_n' has an LLM judge compare competing results and return the winner with a rationale — with worktree isolation only the winning worker's branch is auto-merged, losing branches are preserved unmerged."
                },
                "isolation": {
                    "type": "string",
                    "enum": ["shared", "worktree"],
                    "default": "worktree",
                    "description": "'worktree' (default) creates an isolated git worktree + branch and unique DEV port per worker so concurrent file edits and local servers cannot conflict; successful branches are merged sequentially into the parent workspace when auto_merge is true. 'shared' runs workers in the parent workspace (prefer for read-only/research fan-out)."
                },
                "auto_merge": {
                    "type": "boolean",
                    "default": true,
                    "description": "When isolation is worktree, commit each successful worker's changes and merge its branch into the parent workspace one-at-a-time. On conflict the merge is aborted, the branch is preserved, and the conflict paths are reported. If the parent workspace has uncommitted changes, auto-merge is skipped entirely (branches preserved) so the user's in-flight work is never clobbered. Ignored for shared isolation."
                },
                "allow_shared_fallback": {
                    "type": "boolean",
                    "default": false,
                    "description": "When isolation is worktree and worktree creation fails, allow falling back to the shared parent workspace. Default false (fail the tool instead)."
                },
                "workers_timeout_secs": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Overall wall-clock budget in seconds for all workers to complete. When the budget elapses, still-running workers are cancelled and their partial results are returned. 0 disables the timeout. Defaults to 1800 (overridable via SEN_WORKERS_TIMEOUT_SECS)."
                }
            },
            "required": ["tasks"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let parsed: SpawnArgs = match serde_json::from_value(args) {
            Ok(p) => p,
            Err(err) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("invalid arguments: {err}")),
                });
            }
        };

        if parsed.tasks.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("`tasks` must contain at least one entry".to_string()),
            });
        }

        if parsed.tasks.len() > MAX_WORKERS_PER_CALL {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "`tasks` exceeds the limit of {MAX_WORKERS_PER_CALL} workers per call ({} requested); \
                     split the work into multiple spawn_workers calls",
                    parsed.tasks.len()
                )),
            });
        }

        let parent_ctx = current_session_context();
        let parent_session_id = parent_ctx
            .as_ref()
            .map(|c| c.session_id.clone())
            .unwrap_or_default();
        let parent_workspace_dir = parent_ctx.map(|c| c.workspace_dir);
        let parent_tool_use_id = current_tool_call_id().unwrap_or_default();

        let supervisor = match ensure_supervisor() {
            Ok(s) => s,
            Err(err) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(err.to_string()),
                });
            }
        };

        let parent_draft = take_parent_draft_channel();

        let run_ctx = WorkerRunContext {
            config: Arc::clone(&self.config),
            live_config: self.live_config.clone(),
            parent_workspace_dir: parent_workspace_dir.clone(),
            parent_permission_mode: crate::gateway::ws::desktop::scoped_permission_mode_opt(),
            parent_cost_ctx:
                crate::agent::reward::cost_tracking::current_tool_loop_cost_tracking_context(),
        };

        let base_workspace = parent_workspace_dir
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());

        let mut worktrees: Vec<Option<WorktreeInfo>> =
            (0..parsed.tasks.len()).map(|_| None).collect();
        let mut isolation_notes: Vec<String> = Vec::new();
        if parsed.isolation == Isolation::Worktree {
            let allow_shared_fallback = parsed.allow_shared_fallback;
            match base_workspace.as_ref() {
                Some(base) => {
                    for idx in 0..parsed.tasks.len() {
                        match create_worker_worktree(base, idx).await {
                            Ok(info) => worktrees[idx] = Some(info),
                            Err(err) => {
                                if allow_shared_fallback {
                                    isolation_notes.push(format!(
                                        "worker #{idx}: worktree isolation unavailable ({err}); \
                                         running in the shared workspace instead"
                                    ));
                                } else {
                                    if let Some(first) = worktrees.iter().flatten().next() {
                                        let base_lock = crate::workers::worktree::base_merge_lock(
                                            &first.base,
                                        );
                                        let _base_guard = base_lock.lock().await;
                                        for created in worktrees.iter().flatten() {
                                            let _ = remove_worker_worktree(created).await;
                                        }
                                    }
                                    return Ok(ToolResult {
                                        success: false,
                                        output: String::new(),
                                        error: Some(format!(
                                            "worktree isolation failed for worker #{idx}: {err}; \
                                             already-created worktrees were rolled back; \
                                             pass allow_shared_fallback=true to run shared"
                                        )),
                                    });
                                }
                            }
                        }
                    }
                }
                None => {
                    if allow_shared_fallback {
                        isolation_notes.push(
                            "worktree isolation requested but no parent workspace directory is \
                             known; all workers run in the shared workspace"
                                .to_string(),
                        );
                    } else {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(
                                "worktree isolation requested but no workspace directory is known; \
                                 pass allow_shared_fallback=true to run shared"
                                    .into(),
                            ),
                        });
                    }
                }
            }
        }

        let specs: Vec<WorkerSpec> = parsed
            .tasks
            .iter()
            .enumerate()
            .map(|(idx, task)| {
                let title = task
                    .title
                    .clone()
                    .unwrap_or_else(|| derive_title(&task.prompt));
                let workspace_dir = worktrees[idx]
                    .as_ref()
                    .map(|w| w.path.to_string_lossy().into_owned());
                let context =
                    compose_worker_context(task.context.as_deref(), worktrees[idx].as_ref(), idx);
                WorkerSpec {
                    parent_session_id: parent_session_id.clone(),
                    parent_tool_use_id: parent_tool_use_id.clone(),
                    title,
                    prompt: task.prompt.clone(),
                    context,
                    model: task.model.clone(),
                    workspace_dir,
                }
            })
            .collect();

        let handles = match supervisor.admit_and_spawn_batch(specs, parent_draft.clone(), run_ctx) {
            Ok(h) => h,
            Err(err) => {
                if let Some(first) = worktrees.iter().flatten().next() {
                    let base_lock = crate::workers::worktree::base_merge_lock(&first.base);
                    let _base_guard = base_lock.lock().await;
                    for created in worktrees.iter().flatten() {
                        let _ = remove_worker_worktree(created).await;
                    }
                }
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(err),
                });
            }
        };

        let mut cancel_guard = WorkerBatchCancelGuard {
            handles: handles.clone(),
            worktrees: worktrees.clone(),
            disarmed: false,
        };

        let mut waits = Vec::with_capacity(handles.len());
        for h in &handles {
            waits.push(h.wait());
        }

        let timeout_secs = parsed
            .workers_timeout_secs
            .or_else(|| {
                crate::util::get_runtime_var("SEN_WORKERS_TIMEOUT_SECS")
                    .and_then(|v| v.trim().parse::<u64>().ok())
            })
            .unwrap_or(DEFAULT_WORKERS_TIMEOUT_SECS);
        let deadline = (timeout_secs > 0).then(|| {
            tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs)
        });

        let mut results: Vec<WorkerResult> = Vec::with_capacity(waits.len());
        let mut waits_iter = waits.into_iter().enumerate();
        while let Some((idx, w)) = waits_iter.next() {
            let h = &handles[idx];
            let outcome = match deadline {
                Some(dl) => tokio::time::timeout_at(dl, w).await,
                None => Ok(w.await),
            };
            match outcome {
                Ok(Ok(res)) => results.push(res),
                Ok(Err(_)) => {
                    results.push(WorkerResult {
                        worker_id: h.worker_id.clone(),
                        title: h.title.clone(),
                        status: WorkerStatus::Failed,
                        output: String::new(),
                        error: Some("worker future dropped before completion".to_string()),
                        started_at: h.started_at,
                        finished_at: h.finished_at(),
                    });
                }
                Err(_elapsed) => {
                    for hh in &handles {
                        if hh.result_snapshot().is_none() {
                            hh.cancel();
                        }
                    }
                    let grace_deadline = tokio::time::Instant::now()
                        + std::time::Duration::from_secs(10);
                    let remaining: Vec<usize> = std::iter::once(idx)
                        .chain(waits_iter.by_ref().map(|(i, _)| i))
                        .collect();
                    let grace_waits = remaining.iter().map(|&i| {
                        let hh = &handles[i];
                        async move {
                            match tokio::time::timeout_at(grace_deadline, hh.wait()).await {
                                Ok(Ok(result)) => Some(result),
                                _ => hh.result_snapshot(),
                            }
                        }
                    });
                    let grace_results = futures_util::future::join_all(grace_waits).await;
                    for (i, final_result) in remaining.iter().copied().zip(grace_results) {
                        let hh = &handles[i];
                        if let Some(snapshot) = final_result {
                            results.push(snapshot);
                        } else {
                            results.push(WorkerResult {
                                worker_id: hh.worker_id.clone(),
                                title: hh.title.clone(),
                                status: WorkerStatus::Stopped,
                                output: String::new(),
                                error: Some(format!(
                                    "worker timed out after {timeout_secs}s budget and was \
                                     cancelled"
                                )),
                                started_at: hh.started_at,
                                finished_at: hh.finished_at(),
                            });
                        }
                    }
                    break;
                }
            }
        }

        cancel_guard.disarm();

        let mut change_reports: Vec<Option<String>> = Vec::with_capacity(results.len());
        for wt in &worktrees {
            match wt {
                Some(info) => change_reports.push(Some(worktree_change_report(info).await)),
                None => change_reports.push(None),
            }
        }

        if let Some(tx) = parent_draft.as_ref() {
            let _ = tx
                .send(DraftEvent::ParentResumed {
                    reason: format!("workers_completed({})", results.len()),
                })
                .await;
        }

        let mut best_of_n_winner: Option<usize> = None;
        let mut aggregated = match parsed.merge_strategy {
            MergeStrategy::BestOfN => {
                let (winner, text) = self.judge_best_of_n(&results, &change_reports).await;
                best_of_n_winner = winner;
                text
            }
            _ => {
                self.aggregate_results(&results, &change_reports, parsed.merge_strategy)
                    .await
            }
        };
        if !isolation_notes.is_empty() {
            aggregated.push_str("\n\nIsolation notes:\n");
            for note in &isolation_notes {
                aggregated.push_str(&format!("- {note}\n"));
            }
        }

        let mut merge_failed = false;
        if parsed.isolation == Isolation::Worktree && parsed.auto_merge {
            let base_lock = worktrees
                .iter()
                .flatten()
                .next()
                .map(|info| crate::workers::worktree::base_merge_lock(&info.base));
            let _base_guard = match base_lock.as_ref() {
                Some(lock) => Some(lock.lock().await),
                None => None,
            };
            let parent_dirty = match base_workspace.as_ref() {
                Some(base) => parent_workspace_is_dirty(base).await,
                None => Ok(false),
            };
            match parent_dirty {
                Ok(true) => {
                    let preserved = preserve_all_worktrees(&worktrees).await;
                    aggregated.push_str(
                        "\n\n## Auto-merge\nSkipped: the parent workspace has uncommitted changes, \
                         so merging worker branches now could clobber or conflict with your \
                         in-flight work. Worker changes were committed onto their branches and the \
                         worktree directories were cleaned up. Commit or stash your changes, then \
                         merge manually:\n",
                    );
                    aggregated.push_str(&preserved);
                }
                Err(err) => {
                    let preserved = preserve_all_worktrees(&worktrees).await;
                    aggregated.push_str(&format!(
                        "\n\n## Auto-merge\nSkipped: could not determine whether the parent \
                         workspace has uncommitted changes ({err}); treating it as dirty so \
                         your in-flight work is never clobbered. Worker branches were \
                         preserved for manual review:\n"
                    ));
                    aggregated.push_str(&preserved);
                }
                Ok(false) => {
                    let merge_report = sequential_merge_worktrees(
                        &worktrees,
                        &results,
                        parsed.merge_strategy,
                        best_of_n_winner,
                    )
                    .await;
                    if merge_report.has_failure {
                        merge_failed = true;
                    }
                    aggregated.push_str("\n\n## Auto-merge (sequential)\n");
                    aggregated.push_str(&merge_report.body);
                }
            }
        } else if parsed.isolation == Isolation::Worktree && !parsed.auto_merge {
            let base_lock = worktrees
                .iter()
                .flatten()
                .next()
                .map(|info| crate::workers::worktree::base_merge_lock(&info.base));
            let _base_guard = match base_lock.as_ref() {
                Some(lock) => Some(lock.lock().await),
                None => None,
            };
            let preserved = preserve_all_worktrees(&worktrees).await;
            aggregated.push_str(
                "\n\n## Auto-merge\nSkipped (auto_merge=false). Worker changes were committed \
                 onto their branches and the worktree directories were cleaned up. Merge the \
                 branches you want manually:\n",
            );
            aggregated.push_str(&preserved);
        }

        let any_failed = merge_failed
            || results
                .iter()
                .any(|r| matches!(r.status, WorkerStatus::Failed | WorkerStatus::Stopped));

        Ok(ToolResult {
            success: !any_failed,
            output: aggregated,
            error: None,
        })
    }
}

impl SpawnWorkersTool {
    async fn aggregate_results(
        &self,
        results: &[WorkerResult],
        change_reports: &[Option<String>],
        strategy: MergeStrategy,
    ) -> String {
        match strategy {
            MergeStrategy::Concat | MergeStrategy::BestOfN => {
                concat_results(results, change_reports)
            }
            MergeStrategy::Summary => {
                match self.llm_merge(results, change_reports, MergePrompt::Summary).await {
                    Some(text) => format!(
                        "{text}\n\n---\nFull worker outputs follow:\n\n{}",
                        concat_results(results, change_reports)
                    ),
                    None => concat_results(results, change_reports),
                }
            }
        }
    }

    async fn judge_best_of_n(
        &self,
        results: &[WorkerResult],
        change_reports: &[Option<String>],
    ) -> (Option<usize>, String) {
        let completed = results
            .iter()
            .filter(|r| matches!(r.status, WorkerStatus::Completed))
            .count();
        if completed == 1 {
            let winner = results
                .iter()
                .position(|r| matches!(r.status, WorkerStatus::Completed));
            let text = format!(
                "Only one worker completed successfully; it wins by default.\n\n{}",
                concat_results(results, change_reports)
            );
            return (winner, text);
        }
        if completed == 0 {
            let text = format!(
                "No worker completed successfully; nothing to judge or merge.\n\n{}",
                concat_results(results, change_reports)
            );
            return (None, text);
        }
        match self.llm_merge(results, change_reports, MergePrompt::Judge).await {
            Some(text) => {
                let winner = parse_judge_winner(&text, results.len()).filter(|idx| {
                    results
                        .get(*idx)
                        .is_some_and(|r| matches!(r.status, WorkerStatus::Completed))
                });
                let body = format!(
                    "{text}\n\n---\nAll candidate outputs follow for reference:\n\n{}",
                    concat_results(results, change_reports)
                );
                (winner, body)
            }
            None => {
                let body = format!(
                    "Best-of-n judge unavailable; no winner selected. All candidate outputs \
                     follow:\n\n{}",
                    concat_results(results, change_reports)
                );
                (None, body)
            }
        }
    }

    async fn llm_merge(
        &self,
        results: &[WorkerResult],
        change_reports: &[Option<String>],
        prompt_kind: MergePrompt,
    ) -> Option<String> {
        let model = crate::providers::resolve_default_model(&self.config).ok()?;
        let provider_raw = self
            .config
            .default_provider
            .clone()
            .unwrap_or_else(|| "openrouter".to_string());
        let provider_name =
            crate::providers::resolve_runtime_provider_name(&provider_raw, &self.config);
        let runtime_options = crate::providers::ProviderRuntimeOptions {
            auth_profile_override: None,
            provider_api_url: self.config.api_url.clone(),
            sen_dir: self
                .config
                .config_path
                .parent()
                .map(std::path::PathBuf::from),
            secrets_encrypt: self.config.secrets.encrypt,
            reasoning_enabled: self.config.runtime.reasoning_enabled,
            reasoning_effort: self.config.runtime.reasoning_effort.clone(),
            provider_timeout_secs: Some(self.config.provider_timeout_secs),
            extra_headers: crate::providers::merged_extra_headers_for_config(&self.config),
            api_path: self.config.api_path.clone(),
            provider_max_tokens: self.config.provider_max_tokens,
            model_context_windows: self.config.model_context_windows.clone(),
            model_providers: self.config.model_providers.clone(),
        };
        let provider = crate::providers::create_resilient_runtime_provider_async(
            provider_name,
            self.config.api_key.clone(),
            self.config.api_url.clone(),
            runtime_options,
        )
        .await
        .ok()?;

        let cap = match prompt_kind {
            MergePrompt::Summary => SUMMARY_INPUT_CAP_CHARS,
            MergePrompt::Judge => JUDGE_INPUT_CAP_CHARS,
        };
        let per_worker_cap = (cap / results.len().max(1)).max(400);
        let mut body = String::new();
        for (idx, r) in results.iter().enumerate() {
            let payload = if r.output.trim().is_empty() {
                r.error.clone().unwrap_or_else(|| "<no output>".to_string())
            } else {
                truncate(&r.output, per_worker_cap)
            };
            body.push_str(&format!(
                "### Worker {} — {} [{}]\n{}\n",
                idx + 1,
                r.title,
                r.status.as_str(),
                payload
            ));
            if let Some(Some(report)) = change_reports.get(idx) {
                body.push_str(&format!("Changed files:\n{report}\n"));
            }
            body.push('\n');
        }

        let prompt = match prompt_kind {
            MergePrompt::Summary => format!(
                "You are aggregating the outputs of parallel worker sub-agents. Write a single \
                 coherent synthesis that preserves every concrete finding, file path, command, \
                 and decision. Note agreements and disagreements between workers. Do not invent \
                 information not present below.\n\n{body}"
            ),
            MergePrompt::Judge => format!(
                "You are judging competing attempts at the SAME task from parallel worker \
                 sub-agents. Pick the single best result. Your FIRST line must be exactly \
                 `WINNER: <n>` where <n> is the 1-based worker number of the winner (for \
                 example `WINNER: 2`), or `WINNER: none` if no attempt succeeded. Then explain \
                 in 2-3 sentences why it wins (correctness, completeness, verification \
                 evidence), and restate the winning result in full so it can be used directly. \
                 If none succeeded, summarize the blockers.\n\n{body}"
            ),
        };

        let temperature = self.config.default_temperature;
        match provider.simple_chat(&prompt, &model, temperature).await {
            Ok(text) if !text.trim().is_empty() => Some(text),
            Ok(_) => None,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "spawn_workers llm merge failed; falling back to concat"
                );
                None
            }
        }
    }
}

fn parse_judge_winner(text: &str, candidates: usize) -> Option<usize> {
    for line in text.lines().take(8) {
        let trimmed = line.trim().trim_start_matches(['*', '#', '`', '>']).trim();
        let Some(rest) = trimmed
            .strip_prefix("WINNER:")
            .or_else(|| trimmed.strip_prefix("Winner:"))
            .or_else(|| trimmed.strip_prefix("winner:"))
        else {
            continue;
        };
        let rest = rest.trim();
        if rest.eq_ignore_ascii_case("none") {
            return None;
        }
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let Ok(n) = digits.parse::<usize>() else {
            return None;
        };
        if n >= 1 && n <= candidates {
            return Some(n - 1);
        }
        return None;
    }
    None
}

fn compose_worker_context(
    task_context: Option<&str>,
    worktree: Option<&WorktreeInfo>,
    _idx: usize,
) -> Option<String> {
    let port = allocate_worker_port();
    let runtime = format!(
        "Runtime isolation: use DEV/server port {port} (env SEN_WORKER_PORT / PORT / \
         DEV_SERVER_PORT preferred). Do not bind the parent workspace's default ports."
    );
    match (task_context, worktree) {
        (Some(ctx), Some(wt)) => Some(format!(
            "{ctx}\n\nYou are working in an isolated git worktree at `{}` on branch `{}`. \
             Make your changes here. The parent runtime will commit and sequentially merge \
             successful worker branches into the main workspace after all workers finish.\n{runtime}",
            wt.path.display(),
            wt.branch
        )),
        (None, Some(wt)) => Some(format!(
            "You are working in an isolated git worktree at `{}` on branch `{}`. Make your \
             changes here. The parent runtime will commit and sequentially merge successful \
             worker branches into the main workspace after all workers finish.\n{runtime}",
            wt.path.display(),
            wt.branch
        )),
        (Some(ctx), None) => Some(format!("{ctx}\n\n{runtime}")),
        (None, None) => Some(runtime),
    }
}

struct MergeReport {
    body: String,
    has_failure: bool,
}

async fn sequential_merge_worktrees(
    worktrees: &[Option<WorktreeInfo>],
    results: &[WorkerResult],
    strategy: MergeStrategy,
    best_of_n_winner: Option<usize>,
) -> MergeReport {
    let _merge_guard = match crate::session::acquire_workspace_exclusive_for_current_session().await
    {
        Some(Ok(g)) => Some(g),
        Some(Err(e)) => {
            return MergeReport {
                body: format!(
                    "- merge aborted: could not acquire workspace exclusive lock: {e}"
                ),
                has_failure: true,
            };
        }
        None => None,
    };
    let mut lines: Vec<String> = Vec::new();
    let mut has_failure = false;
    for (idx, wt_opt) in worktrees.iter().enumerate() {
        let Some(info) = wt_opt else {
            lines.push(format!(
                "- worker #{idx}: skipped merge (no worktree; ran shared or worktree create failed)"
            ));
            continue;
        };
        let status = results.get(idx).map(|r| r.status).unwrap_or(WorkerStatus::Failed);
        if !matches!(status, WorkerStatus::Completed) {
            let salvage = salvage_worktree(info).await;
            lines.push(format!(
                "- `{}`: skipped merge (worker status={}); {}",
                info.branch,
                status.as_str(),
                salvage.note
            ));
            continue;
        }
        if strategy == MergeStrategy::BestOfN {
            match best_of_n_winner {
                Some(winner) if winner == idx => {}
                Some(_) => {
                    let salvage = salvage_worktree(info).await;
                    lines.push(format!(
                        "- `{}`: not the judged winner; branch preserved unmerged; {}",
                        info.branch, salvage.note
                    ));
                    continue;
                }
                None => {
                    let salvage = salvage_worktree(info).await;
                    let hint = if salvage.retained {
                        String::new()
                    } else {
                        format!(" — review and merge manually (`git merge {}`)", info.branch)
                    };
                    lines.push(format!(
                        "- `{}`: judge verdict unavailable; branch preserved unmerged; {}{hint}",
                        info.branch, salvage.note
                    ));
                    continue;
                }
            }
        }
        match commit_and_merge_worker(info).await {
            Ok(msg) => lines.push(format!("- `{}`: {msg}", info.branch)),
            Err(err) => {
                has_failure = true;
                lines.push(format!("- `{}`: MERGE FAILED — {err}", info.branch));
            }
        }
    }
    if lines.is_empty() {
        lines.push("- (no worktree merges attempted)".to_string());
    }
    MergeReport {
        body: lines.join("\n"),
        has_failure,
    }
}

async fn preserve_all_worktrees(worktrees: &[Option<WorktreeInfo>]) -> String {
    let mut lines = String::new();
    for info in worktrees.iter().flatten() {
        let salvage = salvage_worktree(info).await;
        if salvage.retained {
            lines.push_str(&format!("- `{}`: {}\n", info.branch, salvage.note));
        } else {
            lines.push_str(&format!(
                "- `{}`: {} — merge with `git merge {}`\n",
                info.branch, salvage.note, info.branch
            ));
        }
    }
    if lines.is_empty() {
        lines.push_str("- (no worktrees to preserve)\n");
    }
    lines
}

fn concat_results(results: &[WorkerResult], change_reports: &[Option<String>]) -> String {
    results
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let header = format!("## {} ({}) [{}]", r.title, r.worker_id, r.status.as_str());
            let body = if r.output.trim().is_empty() {
                r.error.clone().unwrap_or_else(|| "<no output>".to_string())
            } else {
                r.output.clone()
            };
            let changes = match change_reports.get(idx).and_then(|c| c.clone()) {
                Some(report) => format!("\n\nChanged files:\n{report}"),
                None => String::new(),
            };
            format!("{header}\n{body}{changes}")
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

fn derive_title(prompt: &str) -> String {
    let mut iter = prompt.split_whitespace();
    let mut buf = String::new();
    for _ in 0..6 {
        match iter.next() {
            Some(word) => {
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str(word);
            }
            None => break,
        }
    }
    if buf.chars().count() > 60 {
        buf = buf.chars().take(60).collect();
        buf.push('…');
    }
    if buf.is_empty() {
        "worker".to_string()
    } else {
        buf
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
