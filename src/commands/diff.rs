// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /diff command — show pending git changes in the workspace.

use super::registry::{CommandCategory, CommandContext, CommandResult, SlashCommand};
use std::sync::Arc;

/// Slash command metadata; execution is wired via [`handle`] in `ServiceContainer`.
#[allow(dead_code)]
pub fn command() -> SlashCommand {
    SlashCommand {
        name: "diff".into(),
        aliases: vec![],
        description: "Show pending git changes in the workspace".into(),
        usage: "/diff [git diff args]".into(),
        category: CommandCategory::General,
        hidden: false,
        requires_interactive: false,
        remote_safe: true,
        handler: Arc::new(|ctx| Box::pin(handle(ctx))),
    }
}

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("diff");
    if !ctx.args.is_empty() {
        cmd.args(&ctx.args);
    }
    let output = cmd.current_dir(&ctx.cwd).output().await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if out.status.success() {
                let text = if stdout.is_empty() && stderr.is_empty() {
                    "No changes.".to_string()
                } else if !stdout.is_empty() {
                    stdout.into_owned()
                } else {
                    stderr.into_owned()
                };
                CommandResult::ok(text)
            } else {
                CommandResult::err(format!(
                    "git diff failed (exit {}): {}",
                    out.status.code().unwrap_or(-1),
                    if stderr.is_empty() {
                        stdout.into_owned()
                    } else {
                        stderr.into_owned()
                    }
                ))
            }
        }
        Err(e) => CommandResult::err(format!("Failed to run git diff: {e}")),
    }
}
