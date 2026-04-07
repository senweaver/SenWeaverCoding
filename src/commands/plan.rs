// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /plan command — mirrors claude-code-typescript-src`commands/plan/`.
// Toggle plan mode on/off.

use super::registry::{CommandContext, CommandResult};
use crate::agent::coding_mode::CodingMode;

pub async fn handle(_ctx: CommandContext) -> CommandResult {
    // Unified with CodingMode: toggle between Plan and Vibe
    if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
        let current = *svc.coding_mode.read();
        if current == CodingMode::Plan {
            *svc.coding_mode.write() = CodingMode::Vibe;
            CommandResult::ok("Plan mode disabled. Switched to **vibe** mode — all tools are now available.")
        } else {
            *svc.coding_mode.write() = CodingMode::Plan;
            CommandResult::ok("Plan mode enabled. Only read-only tools will be available. Use `/plan` or `/mode vibe` to exit.")
        }
    } else {
        // Fallback to legacy bootstrap state
        if let Ok(bs) = std::panic::catch_unwind(crate::bootstrap::get_state) {
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
}
