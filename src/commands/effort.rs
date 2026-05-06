// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /effort command — set reasoning effort level.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "effort",
    aliases: &[],
    description: "Set the reasoning effort level (low, medium, high)",
    usage: "/effort <low|medium|high>",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_effort),
});

pub async fn handle_effort(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current = if let Some(svc) = crate::services::try_get_services() {
            svc.runtime_flags.get_effort()
        } else {
            std::env::var("SEN_EFFORT").unwrap_or_else(|_| "medium".to_string())
        };
        return CommandResult::ok(format!(
            "Current reasoning effort: {current}\nUsage: /effort <low|medium|high>"
        ));
    }
    let level = ctx.args[0].to_lowercase();
    match level.as_str() {
        "low" | "medium" | "high" => {
            if let Some(svc) = crate::services::try_get_services() {
                svc.runtime_flags.set_effort(&level);
            }
            CommandResult::ok(format!("Reasoning effort set to: {level}"))
        }
        _ => CommandResult::err(format!(
            "Invalid effort level '{level}'. Use low, medium, or high."
        )),
    }
}
