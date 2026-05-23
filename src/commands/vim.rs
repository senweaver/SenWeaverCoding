// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "vim",
    aliases: &[],
    description: "Toggle vim keybinding mode",
    usage: "/vim",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_vim),
});

pub fn is_vim_enabled() -> bool {
    if let Some(svc) = crate::services::try_get_services() {
        return svc.runtime_flags.get_vim_mode() == "on";
    }
    std::env::var("SEN_VIM_MODE")
        .map(|v| v == "on")
        .unwrap_or(false)
}

pub async fn handle_vim(_ctx: CommandContext) -> CommandResult {
    let current = if let Some(svc) = crate::services::try_get_services() {
        svc.runtime_flags.get_vim_mode()
    } else {
        std::env::var("SEN_VIM_MODE").unwrap_or_else(|_| "off".to_string())
    };
    let new_mode = if current == "on" { "off" } else { "on" };
    if let Some(svc) = crate::services::try_get_services() {
        svc.runtime_flags.set_vim_mode(new_mode);
    }
    if new_mode == "on" {
        CommandResult::ok(
            "Vim keybinding mode enabled.\n\
             Note: Full vim editing (motions, operators, text objects) is available in TUI mode (`sen tui`).\n\
             In the basic REPL, vim mode is limited to a mode indicator.",
        )
    } else {
        CommandResult::ok("Vim keybinding mode disabled. Standard editing restored.")
    }
}
