// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};
use crate::agent::coding_mode::CodingMode;

inventory::submit!(StaticSlashCommand {
    name: "mode",
    aliases: &["md"],
    description: "Switch coding mode (vibe, agent, spec, plan, ask, tdd, debug, architect, pair, context, mvai, harness). Run /mode with no arguments to list all modes.",
    usage: "/mode [name]",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_mode),
});

pub async fn handle_mode(ctx: CommandContext) -> CommandResult {
    let arg = ctx.args.first().map(|s| s.as_str()).unwrap_or("").trim();

    if arg.is_empty() {

        let current = get_current_mode();
        let mut lines = vec![format!("Current mode: **{}**\n", current.display_name())];
        lines.push("Available modes:".to_string());
        for mode in CodingMode::all() {
            let marker = if *mode == current { " (active)" } else { "" };
            lines.push(format!(
                "  /mode {:<10} — {}{}",
                mode.display_name(),
                mode.description(),
                marker,
            ));
        }
        return CommandResult::ok(lines.join("\n"));
    }

    match CodingMode::from_str_loose(arg) {
        Some(new_mode) => {
            let prev = get_current_mode();
            set_current_mode(new_mode);

            if prev == CodingMode::Plan && new_mode != CodingMode::Plan {
                if let Some(svc) = crate::services::try_get_services() {
                    let _ = svc.pending_plan.write().take();
                }
            }

            CommandResult::ok(format!(
                "Switched to **{}** mode: {}",
                new_mode.display_name(),
                new_mode.description(),
            ))
        }
        None => {
            let names: Vec<&str> = CodingMode::all().iter().map(|m| m.display_name()).collect();
            CommandResult::err(format!(
                "Unknown mode '{}'. Available: {}",
                arg,
                names.join(", ")
            ))
        }
    }
}

fn get_current_mode() -> CodingMode {
    match crate::services::try_get_services() {
        Some(svc) => *svc.coding_mode.read(),
        None => CodingMode::default(),
    }
}

fn set_current_mode(mode: CodingMode) {
    if let Some(svc) = crate::services::try_get_services() {
        *svc.coding_mode.write() = mode;
    }
}
