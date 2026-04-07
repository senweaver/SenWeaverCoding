// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /mode command — switch between coding modes.

use super::registry::{CommandContext, CommandResult};
use crate::agent::coding_mode::CodingMode;

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let arg = ctx
        .args
        .first()
        .map(|s| s.as_str())
        .unwrap_or("")
        .trim();

    if arg.is_empty() {
        // Show current mode and list all modes
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

            // Leaving Plan mode via /mode → clear any pending plan so the
            // auto-continue prompt doesn't fire unexpectedly.
            if prev == CodingMode::Plan && new_mode != CodingMode::Plan {
                if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
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
    match std::panic::catch_unwind(crate::services::get_services) {
        Ok(svc) => *svc.coding_mode.read(),
        Err(_) => CodingMode::default(),
    }
}

fn set_current_mode(mode: CodingMode) {
    if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
        *svc.coding_mode.write() = mode;
    }
}
