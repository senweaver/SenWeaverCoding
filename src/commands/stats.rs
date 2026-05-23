// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "stats",
    aliases: &[],
    description: "Show session statistics (tokens, cost, tool calls)",
    usage: "/stats",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_stats),
});

pub async fn handle_stats(_ctx: CommandContext) -> CommandResult {
    if let Some(bs) = crate::bootstrap::try_get_state() {
        let mut info = String::new();
        bs.read(|state| {
            let total_input: u64 = state.model_usage.values().map(|u| u.input_tokens).sum();
            let total_output: u64 = state.model_usage.values().map(|u| u.output_tokens).sum();
            let total_requests: u64 = state.model_usage.values().map(|u| u.request_count).sum();
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let uptime_s = now_ms.saturating_sub(state.start_time_epoch_ms) / 1000;

            info = format!(
                "Session: {}\n\
                 Uptime: {uptime_s}s\n\
                 Total cost: ${:.4}\n\
                 API requests: {total_requests}\n\
                 Input tokens: {total_input}\n\
                 Output tokens: {total_output}\n\
                 Lines: +{} / -{}\n\
                 API time: {}ms\n\
                 Tool time: {}ms",
                state.session_id,
                state.total_cost_usd,
                state.total_lines_added,
                state.total_lines_removed,
                state.total_api_duration_ms,
                state.total_tool_duration_ms,
            );
        });
        CommandResult::ok(info)
    } else {
        CommandResult::ok("Stats unavailable (bootstrap not initialized).")
    }
}
