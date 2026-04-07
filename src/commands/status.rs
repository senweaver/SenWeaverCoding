// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /status command — mirrors claude-code-typescript-src`commands/status/`.
// Shows agent status: model, cost, context usage, active tasks.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(_ctx: CommandContext) -> CommandResult {
    if let Ok(bs) = std::panic::catch_unwind(crate::bootstrap::get_state) {
        let mut info = String::new();
        bs.read(|state| {
            let model = state
                .main_loop_model_override
                .as_deref()
                .unwrap_or("default");
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let uptime_s = (now_ms.saturating_sub(state.start_time_epoch_ms)) / 1000;
            info = format!(
                "Session: {}\nModel: {model}\nCost: ${:.4}\nUptime: {uptime_s}s\nLines +{} / -{}\nAPI calls: {}",
                state.session_id,
                state.total_cost_usd,
                state.total_lines_added,
                state.total_lines_removed,
                state.model_usage.values().map(|u| u.request_count).sum::<u64>(),
            );
        });
        CommandResult::ok(info)
    } else {
        CommandResult::ok("Status unavailable (bootstrap not initialized).")
    }
}
