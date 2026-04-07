// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /config command — mirrors claude-code-typescript-src`commands/config/`.
// View or modify agent configuration.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        return CommandResult::ok("Usage: /config [get|set|list] [key] [value]");
    }
    match ctx.args[0].as_str() {
        "list" => match crate::config::Config::load_or_init().await {
            Ok(cfg) => {
                let provider = cfg.default_provider.as_deref().unwrap_or("(not set)");
                let model = cfg.default_model.as_deref().unwrap_or("(not set)");
                let info = format!(
                    "Configuration:\n  provider: {provider}\n  model: {model}\n  temperature: {}\n  timeout: {}s\n  workspace: {}",
                    cfg.default_temperature,
                    cfg.provider_timeout_secs,
                    cfg.workspace_dir.display(),
                );
                CommandResult::ok(info)
            }
            Err(e) => CommandResult::err(format!("Failed to load config: {e}")),
        },
        "get" => {
            let key = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if key.is_empty() {
                return CommandResult::err("Usage: /config get <key>");
            }
            match crate::config::Config::load_or_init().await {
                Ok(cfg) => {
                    let val = match key {
                        "provider" => cfg
                            .default_provider
                            .clone()
                            .unwrap_or_else(|| "(not set)".to_string()),
                        "model" => cfg
                            .default_model
                            .clone()
                            .unwrap_or_else(|| "(not set)".to_string()),
                        "temperature" => cfg.default_temperature.to_string(),
                        "timeout" => cfg.provider_timeout_secs.to_string(),
                        "workspace" => cfg.workspace_dir.display().to_string(),
                        _ => format!("(unknown key: {key})"),
                    };
                    CommandResult::ok(format!("{key} = {val}"))
                }
                Err(e) => CommandResult::err(format!("Failed to load config: {e}")),
            }
        }
        "set" => {
            let key = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            let val = ctx.args.get(2).map(|s| s.as_str()).unwrap_or("");
            if key.is_empty() || val.is_empty() {
                return CommandResult::err("Usage: /config set <key> <value>");
            }
            match crate::config::Config::load_or_init().await {
                Ok(mut cfg) => {
                    match key {
                        "provider" => cfg.default_provider = Some(val.to_string()),
                        "model" => cfg.default_model = Some(val.to_string()),
                        "temperature" => match val.parse::<f64>() {
                            Ok(t) => cfg.default_temperature = t,
                            Err(_) => {
                                return CommandResult::err(format!("Invalid temperature: {val}"));
                            }
                        },
                        "timeout" => match val.parse::<u64>() {
                            Ok(t) => cfg.provider_timeout_secs = t,
                            Err(_) => {
                                return CommandResult::err(format!("Invalid timeout: {val}"));
                            }
                        },
                        "workspace" => {
                            return CommandResult::err(
                                "Workspace path is resolved at runtime; set SEN_WORKSPACE or switch active workspace instead of /config set workspace.".to_string(),
                            );
                        }
                        _ => {
                            return CommandResult::err(format!("Unknown config key: {key}"));
                        }
                    }
                    match cfg.save().await {
                        Ok(()) => CommandResult::ok(format!("Set {key} = {val} (saved)")),
                        Err(e) => CommandResult::err(format!("Failed to save config: {e}")),
                    }
                }
                Err(e) => CommandResult::err(format!("Failed to load config: {e}")),
            }
        }
        sub => CommandResult::err(format!("Unknown config subcommand: {sub}")),
    }
}
