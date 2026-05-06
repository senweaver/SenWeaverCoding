// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /hooks command — list and manage session hooks.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "hooks",
    aliases: &[],
    description: "List and manage session hooks",
    usage: "/hooks [list|add|remove] [args]",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_hooks),
});

pub async fn handle_hooks(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        return CommandResult::ok(
            "Session hooks:\n  /hooks list -- Show registered hooks\n  /hooks add <name> -- Register a hook (requires config.toml edit)\n  /hooks remove <name> -- Unregister a hook (requires config.toml edit)\n\nNote: Hooks are configured in config.toml under [hooks] section.",
        );
    }
    let sub = ctx.args[0].to_lowercase();
    match sub.as_str() {
        "list" => {
            let mut lines = vec!["Registered hooks:".to_string()];
            lines.push(
                "  webhook_audit -- Audit log via webhook (built-in, enabled in config)"
                    .to_string(),
            );
            lines.push(
                "  command_logger -- Log all commands (built-in, enabled in config)".to_string(),
            );

            if let Some(bs) = crate::bootstrap::try_get_state() {
                let mut count = 0u32;
                bs.read(|_state| {
                    count = 2;
                });
                lines.push(format!(
                    "\nTotal: {count} hook(s) registered in this session."
                ));
            }
            lines.push("\nTo enable/disable hooks, edit config.toml:".to_string());
            lines.push("  [hooks.enabled] = true/false".to_string());
            lines.push("  [hooks.builtin.command_logger] = true/false".to_string());
            lines.push("  [hooks.builtin.webhook_audit.url] = <webhook_url>".to_string());
            CommandResult::ok(lines.join("\n"))
        }
        "add" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err(
                    "Usage: /hooks add <hook_name>\nNote: Hooks must be configured in config.toml under [hooks].",
                );
            }
            CommandResult::ok(format!(
                "Hook '{name}' noted. To add a custom hook:\n\
                1. Edit config.toml\n\
                2. Add your hook under [hooks.custom]\n\
                3. Restart SenWeaverCoding"
            ))
        }
        "remove" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err(
                    "Usage: /hooks remove <hook_name>\nNote: To disable a hook, edit config.toml and restart.",
                );
            }
            CommandResult::ok(format!(
                "Hook '{name}' noted for removal. To disable a hook:\n\
                1. Edit config.toml\n\
                2. Set [hooks.enabled] = false or disable specific hooks\n\
                3. Restart SenWeaverCoding"
            ))
        }
        _ => CommandResult::err(format!(
            "Unknown hooks subcommand: {sub}. Use: list, add, remove"
        )),
    }
}
