// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /context command — mirrors claude-code-typescript-src`commands/context/`.
// Shows current context window usage and loaded context files.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "context",
    aliases: &["ctx"],
    description: "Show context window usage and loaded files",
    usage: "/context",
    category: CommandCategory::General,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_context),
});

pub async fn handle_context(_ctx: CommandContext) -> CommandResult {
    if let Some(bs) = crate::bootstrap::try_get_state() {
        let mut info = String::new();
        bs.read(|state| {
            let total_input: u64 = state.model_usage.values().map(|u| u.input_tokens).sum();
            let total_output: u64 = state.model_usage.values().map(|u| u.output_tokens).sum();
            let model = state
                .main_loop_model_override
                .as_deref()
                .unwrap_or("default");
            info = format!(
                "Model: {model}\nInput tokens: {total_input}\nOutput tokens: {total_output}\nModels used: {}",
                state
                    .model_usage
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        });
        CommandResult::ok(info)
    } else {
        CommandResult::ok("Context info unavailable (bootstrap not initialized).")
    }
}
