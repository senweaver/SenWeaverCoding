// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /review command — request a code review of recent changes.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "review",
    aliases: &[],
    description: "Request a code review of recent changes",
    usage: "/review [focus]",
    category: CommandCategory::General,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_review),
});

pub async fn handle_review(ctx: CommandContext) -> CommandResult {
    let focus = if ctx.args.is_empty() {
        "recent changes".to_string()
    } else {
        ctx.args.join(" ")
    };

    let diff = match std::process::Command::new("git")
        .args(["diff", "--stat", "HEAD~1"])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if stdout.is_empty() {
                match std::process::Command::new("git")
                    .args(["diff", "--stat"])
                    .output()
                {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                    Err(_) => "(no diff available)".to_string(),
                }
            } else {
                stdout
            }
        }
        Err(_) => "(git not available)".to_string(),
    };

    let review_msg = format!(
        "Code review requested for: {focus}\n\nChanged files:\n{diff}\n\
         To get a full review, send a follow-up message asking the agent to review these changes in detail."
    );
    CommandResult::ok(review_msg)
}
