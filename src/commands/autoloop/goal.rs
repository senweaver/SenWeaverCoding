// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use super::super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};
use crate::config::Config;
use crate::memory::blackboard::BlackboardHandle;

inventory::submit!(StaticSlashCommand {
    name: "goal",
    aliases: &[],
    description: "Run a goal autonomously across multiple turns until an independent review accepts the result or the round budget is exhausted (Loop Engineering ODAEI closed loop).",
    usage: "/goal <spec>",
    category: CommandCategory::Tasks,
    hidden: false,
    requires_interactive: false,
    remote_safe: false,
    handler: make_handler!(handle),
});

const DEFAULT_MAX_ROUNDS: u32 = 5;

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let goal = ctx.args.join(" ").trim().to_string();
    if goal.is_empty() {
        return CommandResult::err("Usage: /goal <spec>");
    }

    let Some(svc) = crate::services::try_get_services() else {
        return CommandResult::err("Services not initialized; cannot start a goal loop.");
    };
    let config = (*svc.config()).clone();

    let goal_id = format!("goal-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let blackboard =
        crate::agent::multi_agent_runtime::global_runtime().map(|rt| rt.blackboard.clone());

    let goal_for_task = goal.clone();
    let id_for_task = goal_id.clone();
    crate::runtime::task_manager::spawn_supervised(
        format!("autoloop.goal.{goal_id}"),
        async move {
            run_goal_loop(
                config,
                id_for_task,
                goal_for_task,
                blackboard,
                DEFAULT_MAX_ROUNDS,
            )
            .await;
        },
    );

    CommandResult::ok(format!(
        "Autonomous goal loop started (id: {goal_id}, up to {DEFAULT_MAX_ROUNDS} rounds). \
         Progress is recorded on the blackboard under `goal/{goal_id}`. \
         It stops when an independent review accepts the result or the round budget is exhausted. \
         An engaged emergency stop (kill_all) will halt it."
    ))
}

pub async fn run_goal_loop(
    config: Config,
    goal_id: String,
    goal: String,
    blackboard: Option<BlackboardHandle>,
    max_rounds: u32,
) {
    let temperature = config.default_temperature;
    let daily_cap_cents = config.autonomy.max_cost_per_day_cents;

    let mut config = config;
    let isolation = setup_worktree(&config.workspace_dir, &goal_id).await;
    let mut isolation_note = serde_json::Value::Null;
    if let Some((worktree_path, branch)) = isolation.as_ref() {
        config.workspace_dir = worktree_path.clone();
        isolation_note = serde_json::json!({
            "worktree": worktree_path.display().to_string(),
            "branch": branch,
        });
        tracing::info!(
            target: "commands.autoloop.goal",
            goal_id = %goal_id,
            worktree = %worktree_path.display(),
            branch = %branch,
            "unattended goal loop isolated in a dedicated git worktree",
        );
    }

    write_state(
        &blackboard,
        &goal_id,
        serde_json::json!({
            "goal_id": &goal_id,
            "goal": &goal,
            "status": "running",
            "max_rounds": max_rounds,
            "isolation": isolation_note,
            "started_at": chrono::Utc::now().to_rfc3339(),
        }),
    );

    let mut accepted = false;
    let mut round = 0u32;

    while round < max_rounds {
        round += 1;

        if crate::security::estop::is_kill_all() {
            write_state(
                &blackboard,
                &goal_id,
                serde_json::json!({
                    "goal_id": &goal_id,
                    "status": "halted_estop",
                    "round": round,
                    "halted_at": chrono::Utc::now().to_rfc3339(),
                }),
            );
            return;
        }

        if daily_cap_cents > 0 {
            if let Some(tracker) = crate::cost::tracker::CostTracker::try_get_global() {
                let spent_cents = (tracker.daily_cost_usd() * 100.0).round() as u64;
                if spent_cents >= u64::from(daily_cap_cents) {
                    write_state(
                        &blackboard,
                        &goal_id,
                        serde_json::json!({
                            "goal_id": &goal_id,
                            "status": "halted_cost_budget",
                            "round": round,
                            "daily_cap_cents": daily_cap_cents,
                            "spent_cents": spent_cents,
                            "halted_at": chrono::Utc::now().to_rfc3339(),
                        }),
                    );
                    tracing::warn!(
                        target: "commands.autoloop.goal",
                        goal_id = %goal_id,
                        daily_cap_cents,
                        spent_cents,
                        "daily cost cap reached; halting goal loop",
                    );
                    return;
                }
            }
        }

        let prompt = if round == 1 {
            goal.clone()
        } else {
            format!(
                "Continue working toward the following goal. An independent reviewer judged the \
                 previous attempt incomplete. Identify and close the remaining gaps, then finish.\n\n\
                 Goal:\n{goal}"
            )
        };

        let result = crate::agent::run(
            config.clone(),
            Some(prompt),
            None,
            None,
            temperature,
            Vec::new(),
            false,
            None,
            None,
            None,
        )
        .await;

        let answer = match result {
            Ok(text) => text,
            Err(e) => {
                write_state(
                    &blackboard,
                    &goal_id,
                    serde_json::json!({
                        "goal_id": &goal_id,
                        "status": "error",
                        "round": round,
                        "error": e.to_string(),
                        "finished_at": chrono::Utc::now().to_rfc3339(),
                    }),
                );
                return;
            }
        };

        accepted = goal_round_passed(&goal, &answer).await;

        write_state(
            &blackboard,
            &goal_id,
            serde_json::json!({
                "goal_id": &goal_id,
                "status": if accepted { "accepted" } else { "in_progress" },
                "round": round,
                "max_rounds": max_rounds,
                "answer_preview": answer.chars().take(400).collect::<String>(),
                "updated_at": chrono::Utc::now().to_rfc3339(),
            }),
        );

        if accepted {
            break;
        }
    }

    write_state(
        &blackboard,
        &goal_id,
        serde_json::json!({
            "goal_id": &goal_id,
            "status": if accepted { "completed" } else { "budget_exhausted" },
            "rounds_used": round,
            "max_rounds": max_rounds,
            "finished_at": chrono::Utc::now().to_rfc3339(),
        }),
    );

    tracing::info!(
        target: "commands.autoloop.goal",
        goal_id = %goal_id,
        accepted,
        rounds_used = round,
        "autonomous goal loop finished",
    );
}

async fn goal_round_passed(goal: &str, answer: &str) -> bool {
    match crate::agent::flows::global_critic_context() {
        Some(critic) if critic.config().enabled => {
            match crate::agent::self_assess::critic::IndependentCritic::review_turn(
                &critic, goal, answer,
            )
            .await
            {
                Some(verdict) => !verdict.should_retry,
                None => true,
            }
        }
        _ => true,
    }
}

fn write_state(blackboard: &Option<BlackboardHandle>, goal_id: &str, value: serde_json::Value) {
    if let Some(bb) = blackboard.as_ref() {
        bb.inner()
            .write(format!("goal/{goal_id}"), value, "autoloop_goal", "goal");
    }
}

async fn setup_worktree(workspace: &Path, goal_id: &str) -> Option<(PathBuf, String)> {
    let inside = run_git(workspace, &["rev-parse", "--is-inside-work-tree"]).await?;
    if inside.trim() != "true" {
        return None;
    }

    let branch = format!("sen/goal-{goal_id}");
    let worktree_path = std::env::temp_dir().join(format!("sen-goal-{goal_id}"));
    let worktree_str = worktree_path.to_string_lossy().to_string();

    run_git(
        workspace,
        &["worktree", "add", "-b", &branch, &worktree_str, "HEAD"],
    )
    .await?;

    if worktree_path.is_dir() {
        Some((worktree_path, branch))
    } else {
        None
    }
}

async fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = crate::util::hidden_async_command("git");
    cmd.current_dir(cwd);
    cmd.args(args);
    let output = cmd.output().await.ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        tracing::warn!(
            target: "commands.autoloop.goal",
            args = ?args,
            stderr = %String::from_utf8_lossy(&output.stderr),
            "git command for goal worktree isolation failed",
        );
        None
    }
}
