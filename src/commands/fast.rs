// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /fast command — switch to a faster, cheaper model for simple tasks.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "fast",
    aliases: &[],
    description: "Switch to a faster, cheaper model for simple tasks",
    usage: "/fast",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_fast),
});

pub async fn handle_fast(_ctx: CommandContext) -> CommandResult {
    if let Some(bs) = crate::bootstrap::try_get_state() {
        bs.write(|state| {
            state.main_loop_model_override = Some("fast".to_string());
        });
        CommandResult::ok("Switched to fast model profile. The next turn will use the fast model.")
    } else {
        CommandResult::ok("Switched to fast model profile (bootstrap state not available).")
    }
}
