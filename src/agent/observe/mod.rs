// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod file_watch;
pub mod git;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;

use crate::agent::task_orchestrator::queue::{Task, TaskId, TaskPriority};
use crate::config::Config;
use crate::runtime::task_manager::TaskHandle;

use file_watch::FileWatchConfig;

const OBSERVE_CAPABILITY: &str = "general";

fn guard_untrusted(content: &str) -> Result<String, Vec<String>> {
    use crate::security::prompt_guard::core::{GuardResult, PromptGuard};
    let guard = PromptGuard::new();
    match guard.scan(content) {
        GuardResult::Blocked(reason) => Err(vec![reason]),
        GuardResult::Suspicious(patterns, _) => {
            tracing::warn!(
                target: "agent.observe",
                patterns = ?patterns,
                "observe trigger content flagged as suspicious; wrapping as untrusted data",
            );
            Ok(PromptGuard::sanitize_text(content))
        }
        _ => Ok(PromptGuard::sanitize_text(content)),
    }
}

static FILE_WATCH_HANDLE: OnceLock<Mutex<Option<TaskHandle>>> = OnceLock::new();
static WORKER_STARTED: OnceLock<Mutex<bool>> = OnceLock::new();

fn watch_slot() -> &'static Mutex<Option<TaskHandle>> {
    FILE_WATCH_HANDLE.get_or_init(|| Mutex::new(None))
}

fn worker_flag() -> &'static Mutex<bool> {
    WORKER_STARTED.get_or_init(|| Mutex::new(false))
}

pub fn ensure_task_worker(config: &Config) -> bool {
    let mut started = worker_flag().lock();
    if *started {
        return false;
    }
    let Some(runtime) = crate::agent::multi_agent_runtime::global_runtime() else {
        return false;
    };
    let executor =
        crate::agent::task_orchestrator::worker::agent_run_executor(config.clone());
    runtime.spawn_task_worker(
        vec![OBSERVE_CAPABILITY.to_string()],
        Duration::from_secs(2),
        executor,
    );
    *started = true;
    true
}

pub fn submit_observe_task(
    description: impl Into<String>,
    prompt: impl Into<String>,
) -> Option<TaskId> {
    let runtime = crate::agent::multi_agent_runtime::global_runtime()?;
    let task = Task::new(description, prompt, OBSERVE_CAPABILITY, "observe")
        .with_priority(TaskPriority::Low);
    Some(runtime.task_queue.submit(task))
}

pub fn start_file_watch(
    config: &Config,
    root: PathBuf,
    extensions: Vec<String>,
) -> Result<(), String> {
    if crate::agent::multi_agent_runtime::global_runtime().is_none() {
        return Err(
            "multi-agent runtime is not initialized; cannot start the file-watch trigger".into(),
        );
    }
    ensure_task_worker(config);

    let mut slot = watch_slot().lock();
    if let Some(existing) = slot.as_ref() {
        if !existing.is_finished() {
            return Err("a file-watch trigger is already running; stop it first".into());
        }
    }

    let watch_cfg = FileWatchConfig::new(root.clone())
        .with_extensions(extensions)
        .with_debounce(Duration::from_secs(3));

    let on_change: file_watch::ChangeCallback = Arc::new(move |paths: Vec<PathBuf>| {
        let listed: Vec<String> = paths
            .iter()
            .take(40)
            .map(|p| p.display().to_string())
            .collect();
        let description = format!("file-watch: {} file(s) changed", paths.len());
        let changed_block = match guard_untrusted(&listed.join("\n")) {
            Ok(block) => block,
            Err(reasons) => {
                tracing::warn!(
                    target: "agent.observe",
                    reasons = ?reasons,
                    "file-watch trigger blocked by prompt-injection guard; skipping",
                );
                return;
            }
        };
        let prompt = format!(
            "An automated file-watch trigger detected changes in the workspace. \
             Review the following changed files, and if you find a bug, regression, broken \
             build, or incomplete edit, fix it; otherwise summarize that the changes look fine. \
             Do not make unrelated changes.\n\nChanged files:\n{changed_block}"
        );
        match submit_observe_task(description, prompt) {
            Some(task_id) => tracing::info!(
                target: "agent.observe",
                task_id = %task_id,
                changed = paths.len(),
                "file-watch submitted an observe task",
            ),
            None => tracing::warn!(
                target: "agent.observe",
                "file-watch change detected but no runtime available to submit a task",
            ),
        }
    });

    let handle = file_watch::spawn_file_watch(watch_cfg, on_change);
    *slot = Some(handle);
    Ok(())
}

pub fn stop_file_watch() -> bool {
    let mut slot = watch_slot().lock();
    if let Some(handle) = slot.take() {
        handle.abort();
        true
    } else {
        false
    }
}

pub fn is_watching() -> bool {
    watch_slot()
        .lock()
        .as_ref()
        .map(|h| !h.is_finished())
        .unwrap_or(false)
}

pub async fn trigger_from_git(config: &Config, cwd: &std::path::Path) -> Result<Option<TaskId>, String> {
    if crate::agent::multi_agent_runtime::global_runtime().is_none() {
        return Err("multi-agent runtime is not initialized; cannot submit a git trigger".into());
    }
    ensure_task_worker(config);

    match git::git_change_summary(cwd).await {
        Some(summary) => {
            let summary_block = guard_untrusted(&summary).map_err(|reasons| {
                format!(
                    "git trigger blocked by prompt-injection guard: {}",
                    reasons.join("; ")
                )
            })?;
            let prompt = format!(
                "An automated git trigger detected uncommitted changes. Review them for bugs, \
                 regressions, or incomplete work and fix any problems; otherwise confirm they \
                 look correct. Do not make unrelated changes.\n\n{summary_block}"
            );
            Ok(submit_observe_task("git-trigger: working tree changes", prompt))
        }
        None => Ok(None),
    }
}
