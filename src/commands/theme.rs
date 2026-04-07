// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /theme command — mirrors claude-code-typescript-src`commands/theme/`.
// Change the output style / theme.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current =
            std::env::var("SEN_THEME").unwrap_or_else(|_| "default".to_string());
        return CommandResult::ok(format!(
            "Current theme: {current}\nAvailable themes: default, concise, detailed, formal, code-only\nUsage: /theme <name>"
        ));
    }
    let theme = ctx.args[0].to_lowercase();
    let valid = ["default", "concise", "detailed", "formal", "code-only"];
    if !valid.contains(&theme.as_str()) {
        return CommandResult::err(format!(
            "Unknown theme '{theme}'. Available: {}",
            valid.join(", ")
        ));
    }
    unsafe {
        std::env::set_var("SEN_THEME", &theme);
    }
    CommandResult::ok(format!("Theme set to: {theme}"))
}
