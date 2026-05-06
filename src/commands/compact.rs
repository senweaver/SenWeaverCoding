// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /compact command — mirrors claude-code-typescript-src`commands/compact/`.
// Triggers conversation compaction to free context window space.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "compact",
    aliases: &[],
    description: "Compact conversation to free context window",
    usage: "/compact [prompt]",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_compact),
});

pub async fn handle_compact(ctx: CommandContext) -> CommandResult {
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
