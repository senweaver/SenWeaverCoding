// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "clear",
    aliases: &[],
    description: "Clear the terminal screen",
    usage: "/clear",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: true,
    remote_safe: false,
    handler: make_handler!(handle_clear),
});

pub async fn handle_clear(_ctx: CommandContext) -> CommandResult {
    print!("\x1B[2J\x1B[H");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    CommandResult::ok("Screen cleared.")
}
