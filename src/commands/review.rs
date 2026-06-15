// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
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

const MAX_REVIEW_DIFF_CHARS: usize = 60_000;

fn git_output(args: &[&str], cwd: &std::path::Path) -> Option<String> {
    let output = crate::util::hidden_sync_command("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() { None } else { Some(text) }
}

pub async fn handle_review(ctx: CommandContext) -> CommandResult {
    let focus = if ctx.args.is_empty() {
        "recent changes".to_string()
    } else {
        ctx.args.join(" ")
    };

    let stat = git_output(&["diff", "--stat"], &ctx.cwd)
        .or_else(|| git_output(&["diff", "--stat", "HEAD~1"], &ctx.cwd))
        .unwrap_or_else(|| "(no diff stat available)".to_string());
    let mut diff = git_output(&["diff"], &ctx.cwd)
        .or_else(|| git_output(&["diff", "HEAD~1"], &ctx.cwd))
        .unwrap_or_default();
    if diff.len() > MAX_REVIEW_DIFF_CHARS {
        let mut cut = MAX_REVIEW_DIFF_CHARS;
        while cut > 0 && !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
        diff.push_str("\n... (diff truncated for length; inspect remaining files with git tools)");
    }

    if diff.trim().is_empty() {
        return CommandResult::ok(format!(
            "No uncommitted or recent changes found to review (focus: {focus})."
        ));
    }

    let followup = format!(
        "Please perform a thorough code review of the following changes.\n\
         Review focus: {focus}\n\n\
         Summary of changed files:\n{stat}\n\
         Full diff:\n```diff\n{diff}\n```\n\n\
         Evaluate correctness, edge cases, error handling, security, and consistency \
         with the surrounding codebase. Point out concrete issues with file and line \
         references, then summarize overall risk."
    );
    CommandResult::ok_with_followup(
        format!("Submitting code review request (focus: {focus})..."),
        followup,
    )
}
