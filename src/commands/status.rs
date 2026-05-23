// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "status",
    aliases: &["stat"],
    description: "Show agent status: model, cost, context usage",
    usage: "/status",
    category: CommandCategory::General,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_status),
});

pub async fn handle_status(_ctx: CommandContext) -> CommandResult {
    if let Some(bs) = crate::bootstrap::try_get_state() {
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
