// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /fast command — switch to a faster, cheaper model for simple tasks.

use super::registry::{CommandCategory, CommandContext, CommandResult, SlashCommand};
use std::sync::Arc;

/// Slash command metadata; execution is wired via [`handle`] in `ServiceContainer`.
#[allow(dead_code)]
pub fn command() -> SlashCommand {
    SlashCommand {
        name: "fast".into(),
        aliases: vec![],
        description: "Switch to a faster, cheaper model for simple tasks".into(),
        usage: "/fast".into(),
        category: CommandCategory::Configuration,
        hidden: false,
        requires_interactive: false,
        remote_safe: true,
        handler: Arc::new(|ctx| Box::pin(handle(ctx))),
    }
}

pub async fn handle(_ctx: CommandContext) -> CommandResult {
    if let Ok(bs) = std::panic::catch_unwind(crate::bootstrap::get_state) {
        bs.write(|state| {
            state.main_loop_model_override = Some("fast".to_string());
        });
        CommandResult::ok("Switched to fast model profile. The next turn will use the fast model.")
    } else {
        CommandResult::ok("Switched to fast model profile (bootstrap state not available).")
    }
}
