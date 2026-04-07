// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /color command — set terminal color mode.

use super::registry::{CommandCategory, CommandContext, CommandResult, SlashCommand};
use std::sync::Arc;

/// Slash command metadata; execution is wired via [`handle`] in `ServiceContainer`.
#[allow(dead_code)]
pub fn command() -> SlashCommand {
    SlashCommand {
        name: "color".into(),
        aliases: vec![],
        description: "Set color mode (auto, always, never)".into(),
        usage: "/color <auto|always|never>".into(),
        category: CommandCategory::Configuration,
        hidden: false,
        requires_interactive: false,
        remote_safe: true,
        handler: Arc::new(|ctx| Box::pin(handle(ctx))),
    }
}

pub async fn handle(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current = std::env::var("SEN_COLOR").unwrap_or_else(|_| "auto".to_string());
        return CommandResult::ok(format!(
            "Color mode: {current}\nUsage: /color <auto|always|never>"
        ));
    }
    let mode = ctx.args[0].to_lowercase();
    match mode.as_str() {
        "auto" | "always" | "never" => {
            // SAFETY: process-global env mutation; matches other CLI tools that toggle ANSI.
            unsafe {
                std::env::set_var("SEN_COLOR", &mode);
            }
            CommandResult::ok(format!("Color mode set to: {mode}"))
        }
        _ => CommandResult::err(format!(
            "Invalid color mode '{mode}'. Use auto, always, or never."
        )),
    }
}
