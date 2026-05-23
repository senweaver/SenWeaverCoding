// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "color",
    aliases: &[],
    description: "Set color mode (auto, always, never)",
    usage: "/color <auto|always|never>",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_color),
});

pub async fn handle_color(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        let current = if let Some(svc) = crate::services::try_get_services() {
            svc.runtime_flags.get_color_mode()
        } else {
            std::env::var("SEN_COLOR").unwrap_or_else(|_| "auto".to_string())
        };
        return CommandResult::ok(format!(
            "Color mode: {current}\nUsage: /color <auto|always|never>"
        ));
    }
    let mode = ctx.args[0].to_lowercase();
    match mode.as_str() {
        "auto" | "always" | "never" => {
            if let Some(svc) = crate::services::try_get_services() {
                svc.runtime_flags.set_color_mode(&mode);
            }
            CommandResult::ok(format!("Color mode set to: {mode}"))
        }
        _ => CommandResult::err(format!(
            "Invalid color mode '{mode}'. Use auto, always, or never."
        )),
    }
}
