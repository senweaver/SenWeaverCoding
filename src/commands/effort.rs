// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /effort command — set reasoning effort level.

use super::registry::{CommandCategory, CommandContext, CommandResult, SlashCommand};
use std::sync::Arc;

/// Slash command metadata; execution is wired via [`handle`] in `ServiceContainer`.
#[allow(dead_code)]
pub fn command() -> SlashCommand {
    SlashCommand {
        name: "effort".into(),
        aliases: vec![],
        description: "Set the reasoning effort level (low, medium, high)".into(),
        usage: "/effort <low|medium|high>".into(),
        category: CommandCategory::Configuration,
        hidden: false,
        requires_interactive: false,
        remote_safe: true,
        handler: Arc::new(|ctx| Box::pin(handle(ctx))),
    }
}

pub async fn handle(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current = std::env::var("SEN_EFFORT").unwrap_or_else(|_| "medium".to_string());
        return CommandResult::ok(format!(
            "Current reasoning effort: {current}\nUsage: /effort <low|medium|high>"
        ));
    }
    let level = ctx.args[0].to_lowercase();
    match level.as_str() {
        "low" | "medium" | "high" => {
            // SAFETY: no other threads read this env var concurrently in a racy way;
            // it's only checked at the start of each agent turn.
            unsafe { std::env::set_var("SEN_EFFORT", &level) };
            CommandResult::ok(format!("Reasoning effort set to: {level}"))
        }
        _ => CommandResult::err(format!(
            "Invalid effort level '{level}'. Use low, medium, or high."
        )),
    }
}
