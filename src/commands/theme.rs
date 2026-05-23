// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "theme",
    aliases: &[],
    description: "Change output theme",
    usage: "/theme [name]",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_theme),
});

pub async fn handle_theme(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current = crate::util::get_env_var("SEN_THEME").unwrap_or_else(|| "default".to_string());
        return CommandResult::ok(format!(
            "Current theme: {current}\nAvailable themes: default, concise, detailed, formal, code-only\nUsage: /theme <name>"
        ));
    }
    let theme = ctx.args[0].to_lowercase();
    let valid = ["default", "concise", "detailed", "formal", "code-only"];
    if !valid.contains(&theme.as_str()) {
        return CommandResult::err(format!(
            "Unknown theme '{theme}'. Available: {}",
            valid.join(", ")
        ));
    }

    crate::util::set_env_var("SEN_THEME", &theme);
    CommandResult::ok(format!("Theme set to: {theme}"))
}
