// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /resume command — mirrors claude-code-typescript-src`commands/resume/`.
// Resume a previous conversation session.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let cwd = std::env::current_dir().unwrap_or_default();

    match ctx.args.first().map(|s| s.as_str()) {
        Some("list") | None => {
            // List available sessions (no ID provided)
            match crate::cli::bg::list_sessions_sync(&cwd) {
                Ok(sessions) if sessions.is_empty() => {
                    CommandResult::ok("No previous sessions found. Start a new session first.")
                }
                Ok(sessions) => {
                    let mut lines = vec!["Available sessions (use /resume <id>):".to_string()];
                    for s in sessions.iter().take(10) {
                        lines.push(format!(
                            "  {} | {} | {} | {}",
                            s.id,
                            s.status,
                            s.started_at,
                            s.cwd.display()
                        ));
                    }
                    CommandResult::ok(lines.join("\n"))
                }
                Err(e) => CommandResult::err(format!("Failed to list sessions: {e}")),
            }
        }
        Some(id) => {
            // Resume a specific session
            let session_file = cwd
                .join(".senweavercoding")
                .join("sessions")
                .join(format!("{}.json", id));

            if !session_file.exists() {
                return CommandResult::err(format!(
                    "Session '{}' not found. Use /resume list to see available sessions.",
                    id
                ));
            }

            // Check if session has a state file
            let state_file = cwd
                .join(".senweavercoding")
                .join("sessions")
                .join(format!("{}.state.json", id));

            if state_file.exists() {
                CommandResult::ok(format!(
                    "Ready to resume session: {}\nSession state file: {}",
                    id,
                    state_file.display()
                ))
            } else {
                CommandResult::ok(format!(
                    "Session '{}' found (no state file). To resume, run: sen agent --continue",
                    id
                ))
            }
        }
    }
}
