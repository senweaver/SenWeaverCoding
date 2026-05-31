// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "help",
    aliases: &["?"],
    description: "Show available commands and usage",
    usage: "/help [command]",
    category: CommandCategory::General,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_help),
});

pub async fn handle_help(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        if let Some(svc) = crate::services::try_get_services() {
            let commands = svc.command_registry.list(None);
            let mut sorted: Vec<_> = commands.into_iter().collect();
            sorted.sort_by(|a, b| a.name.cmp(&b.name));
            let mut lines = vec!["Available commands:".to_string()];
            for cmd in &sorted {
                lines.push(format!("  /{:<16}  -  {}", cmd.name, cmd.description));
            }
            lines.push(String::new());
            lines.push("Type /help <command> for details.".to_string());
            CommandResult::ok(lines.join("\n"))
        } else {
            CommandResult::ok(STATIC_HELP)
        }
    } else {
        let cmd = &ctx.args[0];
        if let Some(svc) = crate::services::try_get_services() {
            if let Some(slash_cmd) = svc.command_registry.find(cmd) {
                CommandResult::ok(format!("{}\n{}", slash_cmd.usage, slash_cmd.description))
            } else {
                CommandResult::ok(format!(
                    "Unknown command: /{cmd}. Type /help for available commands."
                ))
            }
        } else {
            CommandResult::ok(format!("No detailed help available for /{cmd}."))
        }
    }
}

const STATIC_HELP: &str = "\
Available commands:\n\
  /help  -  Show help\n\
  /quit  -  Exit\n\
  Type /help <command> for details.";
