// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};
use std::path::PathBuf;

use crate::agent::scheduler_runtime::{TaskExecutor, TaskSchedulerRuntime};
use crate::agent::workflow_loader::WorkflowSpec;
use crate::coordinator::delegation::{SubTaskResult, merge_results};

inventory::submit!(StaticSlashCommand {
    name: "workflow",
    aliases: &["wf"],
    description: "Load a workflow file and run its task DAG",
    usage: "/workflow <run|validate> <path>",
    category: CommandCategory::Tasks,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_workflow),
});

pub async fn handle_workflow(ctx: CommandContext) -> CommandResult {
    let sub = ctx.args.first().map(String::as_str).unwrap_or("");
    let path_str = match ctx.args.get(1) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return CommandResult::err("Usage: /workflow <run|validate> <path>"),
    };
    let path = PathBuf::from(path_str);

    match sub {
        "validate" => validate_only(&path),
        "run" | "" => run_workflow(&path).await,
        other => CommandResult::err(format!(
            "Unknown /workflow subcommand '{other}'. Try: run | validate"
        )),
    }
}

fn validate_only(path: &std::path::Path) -> CommandResult {
    match WorkflowSpec::from_file(path) {
        Ok(spec) => match spec.validate() {
            Ok(()) => CommandResult::ok(format!(
                "Workflow '{}' is valid ({} tasks, max_parallel={})",
                spec.name,
                spec.tasks.len(),
                spec.max_parallel
            )),
            Err(e) => CommandResult::err(format!("Validation failed: {e}")),
        },
        Err(e) => CommandResult::err(format!("Failed to load workflow: {e}")),
    }
}

async fn run_workflow(path: &std::path::Path) -> CommandResult {
    let spec = match WorkflowSpec::from_file(path) {
        Ok(s) => s,
        Err(e) => return CommandResult::err(format!("Failed to load workflow: {e}")),
    };
    let scheduler = match spec.build_scheduler() {
        Ok(s) => s,
        Err(e) => return CommandResult::err(format!("Schedule build failed: {e}")),
    };

    let runtime = TaskSchedulerRuntime::new(scheduler);

    let exec: TaskExecutor = std::sync::Arc::new(|task, _ct| {
        let id = task.id.clone();
        let prompt = task.prompt.clone();
        Box::pin(async move { Ok(format!("[{id}] {prompt}")) })
    });

    let started = std::time::Instant::now();
    let outcomes = runtime.run(exec).await;
    let elapsed = started.elapsed();

    let results: Vec<SubTaskResult> = outcomes
        .into_iter()
        .map(|o| SubTaskResult {
            task_id: o.task_id,
            agent_id: "workflow".into(),
            output: o.result,
            success: o.success,
            confidence: None,
            degraded: false,
            reason: None,
        })
        .collect();

    let merged = merge_results(&results, spec.merge_strategy);
    let success_count = results.iter().filter(|r| r.success).count();
    let failure_count = results.len() - success_count;

    let summary = format!(
        "Workflow '{}' finished in {:?}\n\
         Tasks total: {}, succeeded: {}, failed: {}\n\
         ────────────────────────────────\n\
         {merged}",
        spec.name,
        elapsed,
        results.len(),
        success_count,
        failure_count
    );
    CommandResult::ok(summary)
}
