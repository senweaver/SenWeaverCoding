// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /plugin command — mirrors claude-code-typescript-src`commands/plugin/`.
// Manage plugins: list, enable, disable, install.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
                let plugins = svc.plugin_service.list().await;
                if plugins.is_empty() {
                    CommandResult::ok(
                        "No plugins installed. Use /plugin install <path> to add one.",
                    )
                } else {
                    let mut lines = vec!["Installed plugins:".to_string()];
                    for p in &plugins {
                        lines.push(format!(
                            "  {} \u{2014} {} [{:?}]",
                            p.name, p.description, p.status
                        ));
                    }
                    CommandResult::ok(lines.join("\n"))
                }
            } else {
                CommandResult::ok("No plugins installed (services not initialized).")
            }
        }
        "enable" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err("Usage: /plugin enable <name>");
            }
            if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
                if svc.plugin_service.enable(name).await {
                    CommandResult::ok(format!("Plugin '{name}' enabled."))
                } else {
                    CommandResult::err(format!("Plugin '{name}' not found."))
                }
            } else {
                CommandResult::err("Services not initialized.")
            }
        }
        "disable" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err("Usage: /plugin disable <name>");
            }
            if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
                if svc.plugin_service.disable(name).await {
                    CommandResult::ok(format!("Plugin '{name}' disabled."))
                } else {
                    CommandResult::err(format!("Plugin '{name}' not found."))
                }
            } else {
                CommandResult::err("Services not initialized.")
            }
        }
        "install" => {
            let path = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if path.is_empty() {
                return CommandResult::err("Usage: /plugin install <path>\n\
                    Example: /plugin install ./my-plugin");
            }

            // Try to load the plugin from the given path
            let manifest_path = std::path::PathBuf::from(path);
            let manifest_file = if manifest_path.is_dir() {
                manifest_path.join("manifest.toml")
            } else {
                manifest_path.clone()
            };

            if !manifest_file.exists() {
                return CommandResult::err(format!(
                    "Plugin manifest not found: {}\n\
                    Expected manifest.toml at: {}",
                    manifest_file.display(),
                    if manifest_path.is_dir() {
                        manifest_path.join("manifest.toml").display().to_string()
                    } else {
                        manifest_file.display().to_string()
                    }
                ));
            }

            // Register the plugin (the actual loading happens at startup)
            if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
                let info = crate::services::plugin_service::PluginInfo {
                    name: path.to_string(),
                    version: "0.0.0".to_string(),
                    description: format!("Plugin from: {}", path),
                    author: "Unknown".to_string(),
                    source: crate::services::plugin_service::PluginSource::Local {
                        path: path.to_string(),
                    },
                    status: crate::services::plugin_service::PluginStatus::Enabled,
                    provides_tools: vec![],
                    provides_commands: vec![],
                    provides_hooks: vec![],
                };
                svc.plugin_service.register(info).await;
                CommandResult::ok(format!(
                    "Plugin registered from: {}\n\
                    Restart SenWeaverCoding to load the plugin.",
                    path
                ))
            } else {
                CommandResult::ok(format!(
                    "Plugin manifest found at: {}\n\
                    Services not initialized. Start an agent session and try again.",
                    manifest_file.display()
                ))
            }
        }
        _ => CommandResult::err(format!(
            "Unknown plugin subcommand: {subcmd}. Use: list, enable, disable, install"
        )),
    }
}
