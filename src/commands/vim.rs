// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /vim command — toggle vim keybinding mode.

use super::registry::{CommandCategory, CommandContext, CommandResult, SlashCommand};
use std::sync::Arc;

/// Slash command metadata; execution is wired via [`handle`] in `ServiceContainer`.
#[allow(dead_code)]
pub fn command() -> SlashCommand {
    SlashCommand {
        name: "vim".into(),
        aliases: vec![],
        description: "Toggle vim keybinding mode".into(),
        usage: "/vim".into(),
        category: CommandCategory::Configuration,
        hidden: false,
        requires_interactive: false,
        remote_safe: true,
        handler: Arc::new(|ctx| Box::pin(handle(ctx))),
    }
}

pub async fn handle(_ctx: CommandContext) -> CommandResult {
    let current = std::env::var("SEN_VIM_MODE").unwrap_or_else(|_| "off".to_string());
    let new_mode = if current == "on" { "off" } else { "on" };
    // SAFETY: single-threaded CLI entry point; no concurrent env mutation expected.
    unsafe {
        std::env::set_var("SEN_VIM_MODE", new_mode);
    }
    if new_mode == "on" {
        CommandResult::ok(
            "Vim keybinding mode enabled. Use ESC to switch modes, i/a/o to insert, :w to save.",
        )
    } else {
        CommandResult::ok("Vim keybinding mode disabled. Standard editing restored.")
    }
}
