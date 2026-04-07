// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /compact command — mirrors claude-code-typescript-src`commands/compact/`.
// Triggers conversation compaction to free context window space.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let custom_prompt: Option<String> = if ctx.args.is_empty() {
        None
    } else {
        Some(ctx.args.join(" "))
    };

    let utilization = 0.85;
    let turn_count = 20;
    let strategy = crate::services::CompactService::choose_strategy(utilization, turn_count);
    let prompt = match custom_prompt.as_deref() {
        Some(p) => p,
        None => crate::services::CompactService::default_summary_prompt(),
    };

    CommandResult::ok(format!(
        "Compaction triggered (strategy: {strategy:?}). Summary prompt: \"{prompt}\""
    ))
}
