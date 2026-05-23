// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "add-dir",
    aliases: &["add_dir"],
    description: "Add a directory to agent's working context",
    usage: "/add-dir <path>",
    category: CommandCategory::General,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_add_dir),
});

pub async fn handle_add_dir(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        return CommandResult::err(
            "Usage: /add-dir <path> — add a directory to the working context",
        );
    }
    let dir = &ctx.args[0];
    let path = std::path::PathBuf::from(dir);
    if !path.is_dir() {
        return CommandResult::err(format!("Not a directory: {dir}"));
    }

    let abs_path = std::fs::canonicalize(&path).unwrap_or(path);

    if let Some(bs) = crate::bootstrap::try_get_state() {
        bs.write(|state| {
            if !state.session_extra_dirs.contains(&abs_path) {
                state.session_extra_dirs.push(abs_path.clone());
            }
        });
        CommandResult::ok(format!(
            "Added directory to context: {}",
            abs_path.display()
        ))
    } else {
        CommandResult::ok(format!("Directory noted: {}", abs_path.display()))
    }
}
