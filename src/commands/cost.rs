// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /cost command — mirrors claude-code-typescript-src`commands/cost/`.
// Shows session cost and token usage summary.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(_ctx: CommandContext) -> CommandResult {
    if let Ok(bs) = std::panic::catch_unwind(crate::bootstrap::get_state) {
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
