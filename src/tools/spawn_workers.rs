// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
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

struct WorktreeInfo {
    path: PathBuf,
    branch: String,
    base: PathBuf,
}

// Cancels every still-running worker if the parent tool future is dropped
// (turn cancelled, tool timeout, channel closed). Without this the workers
// keep running as orphans until their own 1800s wall clock expires.
struct WorkerBatchCancelGuard {
    handles: Vec<Arc<WorkerHandle>>,
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
         worker gets its own git worktree + branch and unique DEV port (4100+N); set \
         isolation=\"shared\" only for read-only/research fan-out. When isolation is worktree, \
         successful workers are merged sequentially into the parent workspace (auto_merge=true \
         by default) with conflict abort + report. Set merge_strategy=\"best_of_n\" to run \
         competing attempts and have a judge pick the best result before merge. The tool blocks \
         until every worker reaches a terminal state and returns an aggregated result. Workers \
         cannot spawn additional workers."
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
                                "description": "Optional model override; defaults to the parent session's model."
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
                    "description": "How to aggregate worker outputs back to the parent: 'concat' joins full outputs; 'summary' produces an LLM-written synthesis of all workers; 'best_of_n' has an LLM judge compare competing results and return the winner with a rationale."
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
                    "description": "When isolation is worktree, commit each successful worker's changes and merge its branch into the parent workspace one-at-a-time. On conflict the merge is aborted, the branch is preserved, and the conflict paths are reported. Ignored for shared isolation."
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
            // Capture the parent turn's cost-tracking context before spawning so
            // the worker's LLM usage is billed to the same chat session instead
            // of falling through to the un-attributed global tracker.
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
                                    for created in worktrees.iter().flatten() {
                                        let _ = remove_worker_worktree(created).await;
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
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(err),
                });
            }
        };

        let mut cancel_guard = WorkerBatchCancelGuard {
            handles: handles.clone(),
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
        for (idx, w) in waits.into_iter().enumerate() {
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
                    h.cancel();
                    // Bounded-wait for the worker to actually observe cancellation and
                    // record its final result before we snapshot. Snapshotting
                    // immediately raced the worker's own writes, so the sequential
                    // merge could pick up a half-written worktree.
                    let final_result = match tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        h.wait(),
                    )
                    .await
                    {
                        Ok(Ok(result)) => Some(result),
                        _ => h.result_snapshot(),
                    };
                    if let Some(snapshot) = final_result {
                        results.push(snapshot);
                    } else {
                        results.push(WorkerResult {
                            worker_id: h.worker_id.clone(),
                            title: h.title.clone(),
                            status: WorkerStatus::Stopped,
                            output: String::new(),
                            error: Some(format!(
                                "worker timed out after {timeout_secs}s budget and was cancelled"
                            )),
                            started_at: h.started_at,
                            finished_at: h.finished_at(),
                        });
                    }
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

        let mut aggregated = self
            .aggregate_results(&results, &change_reports, parsed.merge_strategy)
            .await;
        if !isolation_notes.is_empty() {
            aggregated.push_str("\n\nIsolation notes:\n");
            for note in &isolation_notes {
                aggregated.push_str(&format!("- {note}\n"));
            }
        }

        let mut merge_failed = false;
        if parsed.isolation == Isolation::Worktree && parsed.auto_merge {
            let merge_report =
                sequential_merge_worktrees(&worktrees, &results, &change_reports).await;
            if merge_report.has_failure {
                merge_failed = true;
            }
            aggregated.push_str("\n\n## Auto-merge (sequential)\n");
            aggregated.push_str(&merge_report.body);
        } else if parsed.isolation == Isolation::Worktree && !parsed.auto_merge {
            aggregated.push_str(
                "\n\n## Auto-merge\nSkipped (auto_merge=false). Review each worker branch and merge manually.\n",
            );
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
            MergeStrategy::Concat => concat_results(results, change_reports),
            MergeStrategy::Summary => {
                match self.llm_merge(results, change_reports, MergePrompt::Summary).await {
                    Some(text) => format!(
                        "{text}\n\n---\nFull worker outputs follow:\n\n{}",
                        concat_results(results, change_reports)
                    ),
                    None => concat_results(results, change_reports),
                }
            }
            MergeStrategy::BestOfN => {
                match self.llm_merge(results, change_reports, MergePrompt::Judge).await {
                    Some(text) => format!(
                        "{text}\n\n---\nAll candidate outputs follow for reference:\n\n{}",
                        concat_results(results, change_reports)
                    ),
                    None => concat_results(results, change_reports),
                }
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
        };
        let provider = crate::providers::create_provider_with_options_async(
            provider_name,
            self.config.api_key.clone(),
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
                 sub-agents. Pick the single best result and explain in 2-3 sentences why it \
                 wins (correctness, completeness, verification evidence). Then restate the \
                 winning result in full so it can be used directly. If none succeeded, say so \
                 and summarize the blockers.\n\n{body}"
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

fn compose_worker_context(
    task_context: Option<&str>,
    worktree: Option<&WorktreeInfo>,
    idx: usize,
) -> Option<String> {
    let port = WORKER_PORT_BASE.saturating_add(idx as u16);
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
    change_reports: &[Option<String>],
) -> MergeReport {
    // Hold the parent workspace exclusive lock across the whole merge sequence
    // so a parent-session turn or another session cannot run concurrent git /
    // file operations in the same working tree mid-merge.
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
            lines.push(format!(
                "- `{}`: skipped merge (worker status={})",
                info.branch,
                status.as_str()
            ));
            continue;
        }
        let changes = change_reports
            .get(idx)
            .and_then(|c| c.as_deref())
            .unwrap_or("");
        if changes.contains("(no file changes)") {
            lines.push(format!(
                "- `{}`: skipped merge (no file changes)",
                info.branch
            ));
            continue;
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

async fn commit_and_merge_worker(info: &WorktreeInfo) -> Result<String, String> {
    let path = info.path.to_string_lossy().to_string();
    let base = info.base.to_string_lossy().to_string();

    let add = crate::util::hidden_async_command("git")
        .args(["-C", &path, "add", "-A"])
        .output()
        .await
        .map_err(|e| format!("git add failed: {e}"))?;
    if !add.status.success() {
        return Err(String::from_utf8_lossy(&add.stderr).trim().to_string());
    }

    // Commit any uncommitted working-tree changes. A worker may also have made
    // its OWN commits (clean tree, nothing staged) — those must still be merged,
    // so "nothing staged" is NOT by itself a reason to skip.
    let staged = crate::util::hidden_async_command("git")
        .args(["-C", &path, "diff", "--cached", "--quiet"])
        .output()
        .await
        .map_err(|e| format!("git diff --cached failed: {e}"))?;
    if !staged.status.success() {
        let msg = format!("sen-worker: {}", info.branch);
        let commit = crate::util::hidden_async_command("git")
            .args(["-C", &path, "commit", "-m", &msg, "--no-verify"])
            .output()
            .await
            .map_err(|e| format!("git commit failed: {e}"))?;
        if !commit.status.success() {
            return Err(String::from_utf8_lossy(&commit.stderr).trim().to_string());
        }
    }

    // Only skip when the branch has no commits beyond the parent's HEAD.
    let ahead = crate::util::hidden_async_command("git")
        .args(["-C", &base, "rev-list", "--count", &format!("HEAD..{}", info.branch)])
        .output()
        .await
        .map_err(|e| format!("git rev-list failed: {e}"))?;
    let ahead_count: u64 = String::from_utf8_lossy(&ahead.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    if !ahead.status.success() || ahead_count == 0 {
        return Ok("no commits ahead of parent; merge skipped".to_string());
    }

    // Pre-validate with merge-tree so a predicted conflict skips the real merge
    // (and its abort dance) entirely, leaving the branch + worktree intact.
    if let Some(conflicts) = merge_tree_conflicts(&base, &info.branch).await {
        if !conflicts.is_empty() {
            return Err(format!(
                "predicted merge conflict in: {} (branch `{}` preserved at {})",
                conflicts.replace('\n', ", "),
                info.branch,
                info.path.display()
            ));
        }
    }

    let merge = crate::util::hidden_async_command("git")
        .args(["-C", &base, "merge", "--no-edit", "--no-ff", &info.branch])
        .output()
        .await
        .map_err(|e| format!("git merge failed to spawn: {e}"))?;
    if merge.status.success() {
        let cleanup = remove_worker_worktree(info).await;
        return Ok(format!(
            "committed and merged into parent workspace{cleanup}"
        ));
    }

    let stderr = String::from_utf8_lossy(&merge.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&merge.stdout).trim().to_string();
    let conflicts = crate::util::hidden_async_command("git")
        .args(["-C", &base, "diff", "--name-only", "--diff-filter=U"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let _ = crate::util::hidden_async_command("git")
        .args(["-C", &base, "merge", "--abort"])
        .output()
        .await;

    let mut detail = String::new();
    if !conflicts.is_empty() {
        detail.push_str("conflict paths: ");
        detail.push_str(&conflicts.replace('\n', ", "));
        detail.push_str("; ");
    }
    if !stderr.is_empty() {
        detail.push_str(&stderr);
    } else if !stdout.is_empty() {
        detail.push_str(&stdout);
    } else {
        detail.push_str("merge conflict; aborted and left worker branch intact");
    }
    detail.push_str(&format!(
        " (branch `{}` preserved at {})",
        info.branch,
        info.path.display()
    ));
    Err(detail)
}

async fn merge_tree_conflicts(base: &str, branch: &str) -> Option<String> {
    // `git merge-tree --write-tree --name-only <HEAD> <branch>` exits non-zero
    // and lists conflicted paths when the merge would conflict. Older gits that
    // do not support the flags return None (skip the pre-check gracefully).
    let out = crate::util::hidden_async_command("git")
        .args([
            "-C",
            base,
            "merge-tree",
            "--write-tree",
            "--name-only",
            "HEAD",
            branch,
        ])
        .output()
        .await
        .ok()?;
    if out.status.success() {
        return Some(String::new());
    }
    let code = out.status.code().unwrap_or(0);
    if code != 1 {
        return None;
    }
    // Output layout: line 1 is the tree OID, then the conflicted-file section,
    // then a BLANK line, then informational messages (Auto-merging / CONFLICT).
    // Take only the file section (stop at the first blank line) so status
    // messages are not mistaken for file paths.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let conflicts: Vec<&str> = stdout
        .lines()
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .collect();
    Some(conflicts.join("\n"))
}

async fn remove_worker_worktree(info: &WorktreeInfo) -> String {
    let base = info.base.to_string_lossy().to_string();
    let path = info.path.to_string_lossy().to_string();
    let removed = crate::util::hidden_async_command("git")
        .args(["-C", &base, "worktree", "remove", "--force", &path])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if removed {
        let _ = crate::util::hidden_async_command("git")
            .args(["-C", &base, "branch", "-D", &info.branch])
            .output()
            .await;
        " (worktree and branch cleaned up)".to_string()
    } else {
        let _ = crate::util::hidden_async_command("git")
            .args(["-C", &base, "worktree", "prune"])
            .output()
            .await;
        " (merged; worktree cleanup deferred to prune)".to_string()
    }
}

async fn create_worker_worktree(base: &Path, idx: usize) -> Result<WorktreeInfo, String> {
    let inside = crate::util::hidden_async_command("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(base)
        .output()
        .await
        .map_err(|e| format!("git not available: {e}"))?;
    if !inside.status.success() {
        return Err("not a git repository".to_string());
    }

    // Worktrees branch from HEAD, so the parent's uncommitted changes are not
    // present by default. Capture the dirty status up front; after creating the
    // worktree we replicate that in-flight work into it (like Cursor copies the
    // current workspace state) so workers operate on what the user actually has.
    let dirty_status = crate::util::hidden_async_command("git")
        .args(["status", "--porcelain"])
        .current_dir(base)
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let batch_id = uuid::Uuid::new_v4().simple().to_string();
    let short_id = &batch_id[..12.min(batch_id.len())];
    let branch = format!("sen-worker/{short_id}-{idx}");
    let worktrees_dir = base.join(".sen").join("worktrees");
    if let Err(e) = tokio::fs::create_dir_all(&worktrees_dir).await {
        return Err(format!("failed to create worktrees dir: {e}"));
    }
    let path = worktrees_dir.join(format!("{short_id}-{idx}"));
    let path_str = path.to_string_lossy().to_string();

    let output = crate::util::hidden_async_command("git")
        .args(["worktree", "add", "-b", &branch, &path_str, "HEAD"])
        .current_dir(base)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("git worktree add failed to spawn: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    if !dirty_status.trim().is_empty() {
        replicate_uncommitted_changes(base, &path, &path_str, &dirty_status).await;
    }

    Ok(WorktreeInfo {
        path,
        branch,
        base: base.to_path_buf(),
    })
}

// Replicate the parent working tree's uncommitted state into a freshly created
// worker worktree: apply the tracked diff-against-HEAD as a patch, then copy any
// untracked files. Best-effort — on failure the worktree simply starts from HEAD
// (logged), never a hard error.
async fn replicate_uncommitted_changes(base: &Path, path: &Path, path_str: &str, status: &str) {
    let diff = crate::util::hidden_async_command("git")
        .args(["diff", "HEAD", "--binary"])
        .current_dir(base)
        .output()
        .await;
    if let Ok(d) = diff {
        if d.status.success() && !d.stdout.is_empty() {
            let patch_path = path.join(".sen-uncommitted.patch");
            if tokio::fs::write(&patch_path, &d.stdout).await.is_ok() {
                let applied = crate::util::hidden_async_command("git")
                    .args([
                        "-C",
                        path_str,
                        "apply",
                        "--whitespace=nowarn",
                        &patch_path.to_string_lossy(),
                    ])
                    .output()
                    .await
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                let _ = tokio::fs::remove_file(&patch_path).await;
                if !applied {
                    tracing::warn!(
                        target: "workers.worktree",
                        "could not replay parent uncommitted diff into worker worktree; \
                         it starts from HEAD for tracked files"
                    );
                }
            }
        }
    }

    for line in status.lines() {
        // Porcelain untracked entries look like `?? path/to/file`.
        let Some(rel) = line.strip_prefix("?? ") else {
            continue;
        };
        let rel = rel.trim().trim_matches('"');
        if rel.is_empty() || rel.ends_with('/') {
            continue;
        }
        let src = base.join(rel);
        let dst = path.join(rel);
        if let Some(parent) = dst.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = tokio::fs::copy(&src, &dst).await {
            tracing::debug!(
                target: "workers.worktree",
                file = %rel,
                error = %e,
                "could not copy untracked file into worker worktree"
            );
        }
    }
}

async fn worktree_change_report(info: &WorktreeInfo) -> String {
    let output = crate::util::hidden_async_command("git")
        .args(["-C", &info.path.to_string_lossy(), "status", "--short"])
        .current_dir(&info.base)
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                "(no file changes)".to_string()
            } else {
                truncate(trimmed, 1_500)
            }
        }
        _ => "(unable to read worktree status)".to_string(),
    }
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
