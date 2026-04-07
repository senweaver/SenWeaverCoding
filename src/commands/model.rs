// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /model command — mirrors claude-code-typescript-src`commands/model/`.
// Switch or display the current model.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current = std::panic::catch_unwind(crate::bootstrap::get_state)
            .ok()
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
    if let Ok(bs) = std::panic::catch_unwind(crate::bootstrap::get_state) {
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
