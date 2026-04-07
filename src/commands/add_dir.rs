// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /add-dir command — mirrors claude-code-typescript-src`commands/add-dir/`.
// Adds additional directories to the agent's working context.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
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

    if let Ok(bs) = std::panic::catch_unwind(crate::bootstrap::get_state) {
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
