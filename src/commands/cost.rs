// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "cost",
    aliases: &[],
    description: "Show session cost and token usage",
    usage: "/cost",
    category: CommandCategory::General,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_cost),
});

pub async fn handle_cost(_ctx: CommandContext) -> CommandResult {
    if let Some(bs) = crate::bootstrap::try_get_state() {
        let mut info = String::new();
        bs.read(|state| {
            let mut lines = vec![format!("Total cost: ${:.4}", state.total_cost_usd)];
            for (model, usage) in &state.model_usage {
                lines.push(format!(
                    "  {model}: ${:.4} ({} requests, {} in/{} out tokens)",
                    usage.total_cost_usd,
                    usage.request_count,
                    usage.input_tokens,
                    usage.output_tokens
                ));
            }
            if state.model_usage.is_empty() {
                lines.push("  No model usage recorded yet.".to_string());
            }
            info = lines.join("\n");
        });
        CommandResult::ok(info)
    } else {
        CommandResult::ok("Cost info unavailable (bootstrap not initialized).")
    }
}
