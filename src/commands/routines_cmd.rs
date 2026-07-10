// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "routines",
    aliases: &["routine"],
    description: "Manage event-driven routines: list, add, remove, reload, reset-cooldowns",
    usage: "/routines [list|add|remove|reload|reset-cooldowns]",
    category: CommandCategory::Tools,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_routines),
});

fn action_label(action: &crate::routines::RoutineAction) -> String {
    match action {
        crate::routines::RoutineAction::Sop { name } => format!("sop:{name}"),
        crate::routines::RoutineAction::Shell { command } => {
            format!("shell:{}", command.chars().take(40).collect::<String>())
        }
        crate::routines::RoutineAction::Message { channel, .. } => {
            format!("message:{channel}")
        }
        crate::routines::RoutineAction::CronJob { job_name } => format!("cron:{job_name}"),
    }
}

pub async fn handle_routines(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            let routines = crate::routines::list_routines();
            if routines.is_empty() {
                return CommandResult::ok(
                    "No routines defined. Add one with /routines add <json>, or edit \
                     routines.toml in your workspace.",
                );
            }
            let mut lines = vec![format!("Routines ({}):", routines.len())];
            for r in &routines {
                lines.push(format!(
                    "  {} \u{2014} {} [{}] patterns={} cooldown={}s{}",
                    r.name,
                    if r.description.is_empty() {
                        "(no description)"
                    } else {
                        &r.description
                    },
                    action_label(&r.action),
                    r.patterns.len(),
                    r.cooldown_secs,
                    if r.enabled { "" } else { " (disabled)" },
                ));
            }
            CommandResult::ok(lines.join("\n"))
        }
        "add" => {
            let json = ctx
                .raw_input
                .find('{')
                .map(|idx| ctx.raw_input[idx..].to_string())
                .unwrap_or_default();
            if json.trim().is_empty() {
                return CommandResult::err(
                    "Usage: /routines add <json>\n\
                     Example: /routines add {\"name\":\"greet\",\"patterns\":[{\"source\":\"telegram\",\
                     \"pattern\":\"hello\"}],\"action\":{\"type\":\"message\",\"channel\":\"telegram\",\
                     \"text\":\"hi\"}}",
                );
            }
            let routine: crate::routines::Routine = match serde_json::from_str(&json) {
                Ok(r) => r,
                Err(e) => return CommandResult::err(format!("Invalid routine JSON: {e}")),
            };
            let name = routine.name.clone();
            match crate::routines::add_routine(routine) {
                Ok(()) => CommandResult::ok(format!(
                    "Routine '{name}' added and saved to routines.toml."
                )),
                Err(e) => CommandResult::err(format!("Failed to add routine: {e}")),
            }
        }
        "remove" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err("Usage: /routines remove <name>");
            }
            match crate::routines::remove_routine(name) {
                Ok(true) => CommandResult::ok(format!(
                    "Routine '{name}' removed and routines.toml updated."
                )),
                Ok(false) => CommandResult::err(format!("Routine '{name}' not found.")),
                Err(e) => CommandResult::err(format!("Failed to remove routine: {e}")),
            }
        }
        "reload" => {
            let count = crate::routines::reload_routines();
            CommandResult::ok(format!("Reloaded {count} routines from routines.toml."))
        }
        "reset-cooldowns" => {
            crate::routines::reset_cooldowns();
            CommandResult::ok("All routine cooldowns reset.")
        }
        _ => CommandResult::err(format!(
            "Unknown routines subcommand: {subcmd}. Use: list, add, remove, reload, \
             reset-cooldowns"
        )),
    }
}
