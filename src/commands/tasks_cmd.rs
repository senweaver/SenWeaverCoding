// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandContext, CommandResult};
use anyhow::Result;

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            match crate::cli::bg::list_sessions_sync(&cwd) {
                Ok(sessions) => {
                    let running: Vec<_> = sessions
                        .iter()
                        .filter(|s| s.status == crate::cli::bg::SessionStatus::Running)
                        .collect();
                    if running.is_empty() {
                        CommandResult::ok("No background tasks running.")
                    } else {
                        let mut lines = vec![format!("{} background task(s):", running.len())];
                        for s in &running {
                            lines.push(format!(
                                "  {} | started: {} | cwd: {}",
                                s.id,
                                s.started_at,
                                s.cwd.display()
                            ));
                        }
                        CommandResult::ok(lines.join("\n"))
                    }
                }
                Err(e) => CommandResult::err(format!("Failed to list tasks: {e}")),
            }
        }
        "kill" => {
            let id = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() {
                return CommandResult::err("Usage: /tasks kill <task_id>");
            }
            let cwd = std::env::current_dir().unwrap_or_default();
            match kill_session_sync(&cwd, id).await {
                Ok(()) => CommandResult::ok(format!("Task '{id}' has been terminated.")),
                Err(e) => CommandResult::err(format!("Failed to kill task: {e}")),
            }
        }
        "inspect" => {
            let id = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() {
                return CommandResult::err("Usage: /tasks inspect <task_id>");
            }
            let cwd = std::env::current_dir().unwrap_or_default();
            match inspect_session(&cwd, id).await {
                Ok(info) => {
                    let lines = [
                        format!("Task ID: {}", info.id),
                        format!("Status: {}", info.status),
                        format!("Started: {}", info.started_at),
                        format!("Last Activity: {}", info.last_activity),
                        format!("CWD: {}", info.cwd.display()),
                        format!("PID: {:?}", info.pid),
                    ];
                    CommandResult::ok(lines.join("\n"))
                }
                Err(e) => CommandResult::err(format!("Failed to inspect task: {e}")),
            }
        }
        _ => CommandResult::err(format!(
            "Unknown tasks subcommand: {subcmd}. Available: list, kill, inspect"
        )),
    }
}

async fn kill_session_sync(workspace: &std::path::Path, session_id: &str) -> Result<()> {
    let session_file = workspace
        .join(".senweavercoding")
        .join("sessions")
        .join(format!("{}.json", session_id));
    if !session_file.exists() {
        anyhow::bail!("Session '{}' not found", session_id);
    }

    let data = tokio::fs::read_to_string(&session_file).await?;
    let info: crate::cli::bg::SessionInfo = serde_json::from_str(&data)?;

    if let Some(pid) = info.pid {
        if !crate::cli::bg::verify_pid_is_sen_with_start(pid, info.pid_start_time) {
            tracing::warn!(
                pid,
                session_id,
                "PID does not match recorded sen process (possible PID reuse)  -  skipping signal"
            );
        } else {
            crate::cli::bg::safe_terminate_pid(pid).await;
            tracing::info!(
                "Sent termination signal to session '{}' (PID {})",
                session_id,
                pid
            );
        }
    }

    let updated = crate::cli::bg::SessionInfo {
        status: crate::cli::bg::SessionStatus::Stopped,
        ..info
    };
    tokio::fs::write(&session_file, serde_json::to_string_pretty(&updated)?).await?;
    Ok(())
}

async fn inspect_session(
    workspace: &std::path::Path,
    session_id: &str,
) -> Result<crate::cli::bg::SessionInfo> {
    let session_file = workspace
        .join(".senweavercoding")
        .join("sessions")
        .join(format!("{}.json", session_id));
    if !session_file.exists() {
        anyhow::bail!("Session '{}' not found", session_id);
    }
    let data = tokio::fs::read_to_string(&session_file).await?;
    let info: crate::cli::bg::SessionInfo = serde_json::from_str(&data)?;
    Ok(info)
}
