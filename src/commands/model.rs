// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "model",
    aliases: &["mdl"],
    description: "Switch or show the current model",
    usage: "/model [name]",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_model),
});

pub async fn handle_model(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current = crate::bootstrap::try_get_state()
            .and_then(|bs| {
                let mut model = None;
                bs.read(|state| {
                    model = state.main_loop_model_override.clone();
                });
                model
            })
            .unwrap_or_else(|| "(default)".to_string());
        return CommandResult::ok(format!("Current model: {current}"));
    }

    let model = ctx.args.join(" ");
    if let Some(bs) = crate::bootstrap::try_get_state() {
        bs.write(|state| {
            state.main_loop_model_override = Some(model.clone());
        });
        CommandResult::ok(format!("Model switched to: {model}"))
    } else {
        CommandResult::ok(format!(
            "Model set to: {model} (bootstrap state not available)"
        ))
    }
}
