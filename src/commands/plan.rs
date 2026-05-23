// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};
use crate::agent::coding_mode::CodingMode;

inventory::submit!(StaticSlashCommand {
    name: "plan",
    aliases: &[],
    description: "Toggle plan mode on/off",
    usage: "/plan",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_plan),
});

pub async fn handle_plan(_ctx: CommandContext) -> CommandResult {

    if let Some(svc) = crate::services::try_get_services() {
        let current = *svc.coding_mode.read();
        if current == CodingMode::Plan {
            *svc.coding_mode.write() = CodingMode::Vibe;
            CommandResult::ok(
                "Plan mode disabled. Switched to **vibe** mode — all tools are now available.",
            )
        } else {
            *svc.coding_mode.write() = CodingMode::Plan;
            CommandResult::ok(
                "Plan mode enabled. Only read-only tools will be available. Use `/plan` or `/mode vibe` to exit.",
            )
        }
    } else if let Some(bs) = crate::bootstrap::try_get_state() {
        let mut was_plan = false;
        bs.read(|state| {
            was_plan = state.has_exited_plan_mode;
        });
        let now_plan = !was_plan;
        bs.write(|state| {
            state.has_exited_plan_mode = !now_plan;
        });
        if now_plan {
            CommandResult::ok("Plan mode enabled. Only read-only tools will be available.")
        } else {
            CommandResult::ok("Plan mode disabled. All tools are now available.")
        }
    } else {
        CommandResult::ok("Plan mode toggled (bootstrap state not available).")
    }
}
