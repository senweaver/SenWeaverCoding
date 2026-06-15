// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
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

fn apply_hook_toggle(
    config: &mut crate::config::Config,
    name: &str,
    enable: bool,
    url: Option<&str>,
) -> Result<String, String> {
    match name {
        "command_logger" => {
            config.hooks.builtin.command_logger = enable;
            if enable {
                config.hooks.enabled = true;
            }
            Ok(format!(
                "Builtin hook 'command_logger' is now {}.",
                if enable { "enabled" } else { "disabled" }
            ))
        }
        "webhook_audit" => {
            if enable {
                if let Some(url) = url {
                    config.hooks.builtin.webhook_audit.url = url.to_string();
                }
                if config.hooks.builtin.webhook_audit.url.trim().is_empty() {
                    return Err(
                        "webhook_audit requires a URL. Usage: /hooks add webhook_audit <url>"
                            .to_string(),
                    );
                }
                config.hooks.enabled = true;
            }
            config.hooks.builtin.webhook_audit.enabled = enable;
            Ok(format!(
                "Builtin hook 'webhook_audit' is now {}{}.",
                if enable { "enabled" } else { "disabled" },
                if enable {
                    format!(" (url: {})", config.hooks.builtin.webhook_audit.url)
                } else {
                    String::new()
                }
            ))
        }
        other => Err(format!(
            "Unknown hook '{other}'. Available builtin hooks: command_logger, webhook_audit"
        )),
    }
}

async fn persist_hook_change(
    name: &str,
    enable: bool,
    url: Option<&str>,
) -> CommandResult {
    let mut config = match crate::config::Config::load_or_init().await {
        Ok(c) => c,
        Err(e) => return CommandResult::err(format!("Failed to load config: {e}")),
    };
    let summary = match apply_hook_toggle(&mut config, name, enable, url) {
        Ok(s) => s,
        Err(e) => return CommandResult::err(e),
    };
    if let Err(e) = config.save().await {
        return CommandResult::err(format!("Failed to write config.toml: {e}"));
    }

    let hooks_section = config.hooks.clone();
    let hot_applied = if let Some(svc) = crate::services::try_get_services() {
        svc.shared_config.mutate(
            move |live| {
                live.hooks = hooks_section;
            },
            vec!["hooks".to_string()],
        );
        true
    } else {
        false
    };

    let effect_note = if hot_applied {
        "Change written to config.toml and hot-applied to the running session."
    } else {
        "Change written to config.toml; restart SenWeaverCoding for it to take effect."
    };
    CommandResult::ok(format!("{summary}\n{effect_note}"))
}

pub async fn handle_hooks(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        return CommandResult::ok(
            "Session hooks:\n  /hooks list -- Show hook configuration\n  /hooks add <name> [url] -- Enable a builtin hook (command_logger | webhook_audit)\n  /hooks remove <name> -- Disable a builtin hook",
        );
    }
    let sub = ctx.args[0].to_lowercase();
    match sub.as_str() {
        "list" => {
            let config = match crate::services::try_get_services() {
                Some(svc) => svc.config(),
                None => match crate::config::Config::load_or_init().await {
                    Ok(c) => std::sync::Arc::new(c),
                    Err(e) => {
                        return CommandResult::err(format!("Failed to load config: {e}"));
                    }
                },
            };
            let hooks = &config.hooks;
            let mut lines = vec![format!(
                "Hooks globally {}.",
                if hooks.enabled { "enabled" } else { "disabled" }
            )];
            lines.push(format!(
                "  command_logger -- Log all commands [{}]",
                if hooks.builtin.command_logger { "on" } else { "off" }
            ));
            lines.push(format!(
                "  webhook_audit -- Audit log via webhook [{}]{}",
                if hooks.builtin.webhook_audit.enabled { "on" } else { "off" },
                if hooks.builtin.webhook_audit.url.is_empty() {
                    String::new()
                } else {
                    format!(" (url: {})", hooks.builtin.webhook_audit.url)
                }
            ));
            lines.push(
                "\nUse /hooks add <name> [url] or /hooks remove <name> to change them."
                    .to_string(),
            );
            CommandResult::ok(lines.join("\n"))
        }
        "add" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err(
                    "Usage: /hooks add <hook_name> [url]\nAvailable: command_logger, webhook_audit",
                );
            }
            let url = ctx.args.get(2).map(|s| s.as_str());
            persist_hook_change(name, true, url).await
        }
        "remove" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err(
                    "Usage: /hooks remove <hook_name>\nAvailable: command_logger, webhook_audit",
                );
            }
            persist_hook_change(name, false, None).await
        }
        _ => CommandResult::err(format!(
            "Unknown hooks subcommand: {sub}. Use: list, add, remove"
        )),
    }
}
