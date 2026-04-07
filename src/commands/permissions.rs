// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /permissions command — show or adjust permission settings.

use super::registry::{CommandCategory, CommandContext, CommandResult, SlashCommand};
use std::sync::Arc;

/// Slash command metadata; execution is wired via [`handle`] in `ServiceContainer`.
#[allow(dead_code)]
pub fn command() -> SlashCommand {
    SlashCommand {
        name: "permissions".into(),
        aliases: vec!["perms".into()],
        description: "Show current permission settings".into(),
        usage: "/permissions [subcommand]".into(),
        category: CommandCategory::Configuration,
        hidden: false,
        requires_interactive: false,
        remote_safe: true,
        handler: Arc::new(|ctx| Box::pin(handle(ctx))),
    }
}

pub async fn handle(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
            let mut lines = vec!["Permission settings:".to_string()];

            let bs_info = std::panic::catch_unwind(crate::bootstrap::get_state)
                .ok()
                .map(|bs| {
                    let mut bypass = false;
                    bs.read(|state| bypass = state.session_bypass_permissions_mode);
                    bypass
                });

            lines.push(format!("  Bypass mode: {}", bs_info.unwrap_or(false)));

            let test_tools = ["shell", "file_write", "file_read"];
            for tool in &test_tools {
                let result = svc.check_tool_policy(tool);
                lines.push(format!(
                    "  Tool '{}': {}",
                    tool,
                    if result { "allowed" } else { "blocked" }
                ));
            }

            CommandResult::ok(lines.join("\n"))
        } else {
            CommandResult::ok("Permission settings unavailable (services not initialized).")
        }
    } else {
        let sub = ctx.args[0].to_lowercase();
        match sub.as_str() {
            "show" | "list" => {
                if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
                    let test_tools =
                        ["shell", "file_write", "file_read", "browser", "glob_search"];
                    let mut lines = vec!["Tool permissions:".to_string()];
                    for tool in &test_tools {
                        let allowed = svc.check_tool_policy(tool);
                        lines.push(format!(
                            "  {}: {}",
                            tool,
                            if allowed { "\u{2713}" } else { "\u{2717}" }
                        ));
                    }
                    CommandResult::ok(lines.join("\n"))
                } else {
                    CommandResult::ok("Services not initialized.")
                }
            }
            "bypass" => {
                if let Ok(bs) = std::panic::catch_unwind(crate::bootstrap::get_state) {
                    bs.write(|state| {
                        state.session_bypass_permissions_mode =
                            !state.session_bypass_permissions_mode;
                    });
                    let mut mode = false;
                    bs.read(|state| mode = state.session_bypass_permissions_mode);
                    CommandResult::ok(format!(
                        "Bypass permissions mode: {}",
                        if mode { "ON" } else { "OFF" }
                    ))
                } else {
                    CommandResult::err("Bootstrap state not available.")
                }
            }
            _ => CommandResult::err(format!(
                "Unknown permissions subcommand: {sub}. Use: show, bypass"
            )),
        }
    }
}
