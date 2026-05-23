// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "metrics",
    aliases: &[],
    description: "Display current agent metrics (Prometheus format)",
    usage: "/metrics",
    category: CommandCategory::Debug,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_metrics),
});

pub async fn handle_metrics(_ctx: CommandContext) -> CommandResult {
    let Some(svc) = crate::services::try_get_services() else {
        return CommandResult::err("Services not initialized");
    };
    let text = svc.agent_metrics.render_prometheus();
    if text.is_empty() {
        CommandResult::ok("(no metrics recorded yet -- run a turn first)")
    } else {
        CommandResult::ok(format!("# sen agent metrics\n{text}"))
    }
}
