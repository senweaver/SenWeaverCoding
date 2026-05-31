// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "config",
    aliases: &["cfg"],
    description: "View or modify agent configuration",
    usage: "/config [get|set|list|export]",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_config),
});

pub async fn handle_config(ctx: CommandContext) -> CommandResult {
    if ctx.args.is_empty() {
        return CommandResult::ok("Usage: /config [get|set|list] [key] [value]");
    }
    match ctx.args[0].as_str() {
        "list" => match crate::config::Config::load_or_init().await {
            Ok(cfg) => {
                let provider = cfg.default_provider.as_deref().unwrap_or("(not set)");
                let model = cfg.default_model.as_deref().unwrap_or("(not set)");
                let api_url = cfg.api_url.as_deref().unwrap_or("(not set)");
                let info = format!(
                    "Configuration:\n\
                     \x20 provider: {provider}\n\
                     \x20 model: {model}\n\
                     \x20 temperature: {}\n\
                     \x20 timeout: {}s\n\
                     \x20 workspace: {}\n\
                     \x20 api_url: {api_url}\n\
                     \x20 gateway.host: {}\n\
                     \x20 gateway.port: {}\n\
                     \x20 memory.backend: {}\n\
                     \x20 memory.auto_save: {}\n\
                     \x20 web_search.enabled: {}\n\
                     \x20 web_search.provider: {}",
                    cfg.default_temperature,
                    cfg.provider_timeout_secs,
                    cfg.workspace_dir.display(),
                    cfg.gateway.host,
                    cfg.gateway.port,
                    cfg.memory.backend,
                    cfg.memory.auto_save,
                    cfg.web_search.enabled,
                    cfg.web_search.provider,
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
                    let val = get_value(&cfg, key);
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
                Ok(mut cfg) => match set_value(&mut cfg, key, val) {
                    Ok(()) => match cfg.save().await {
                        Ok(()) => CommandResult::ok(format!("Set {key} = {val} (saved)")),
                        Err(e) => CommandResult::err(format!("Failed to save config: {e}")),
                    },
                    Err(msg) => CommandResult::err(msg),
                },
                Err(e) => CommandResult::err(format!("Failed to load config: {e}")),
            }
        }
        "export" => {

            let second = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            let format = if second == "--schema" {
                "schema"
            } else if second == "--toml" {
                "toml"
            } else {

                "toml"
            };
            let path_arg = ctx
                .args
                .iter()
                .skip(1)
                .find(|s| !s.starts_with("--"))
                .cloned();

            match format {
                "schema" => match crate::config::schema::export::export_config_schema() {
                    Ok(s) => {
                        if let Some(path) = path_arg {
                            match std::fs::write(&path, &s) {
                                Ok(()) => CommandResult::ok(format!(
                                    "Wrote JSON Schema to {path} ({} bytes)",
                                    s.len()
                                )),
                                Err(e) => CommandResult::err(format!("Failed to write file: {e}")),
                            }
                        } else {

                            let preview = if s.len() > 500 {
                                format!(
                                    "{}\n\n[... {} more chars  -  pass a path argument to save ...]",
                                    &s[..500],
                                    s.len() - 500
                                )
                            } else {
                                s
                            };
                            CommandResult::ok(preview)
                        }
                    }
                    Err(e) => CommandResult::err(format!("Schema export failed: {e}")),
                },
                "toml" => match crate::config::Config::load_or_init().await {
                    Ok(cfg) => {

                        match toml::to_string_pretty(&cfg) {
                            Ok(s) => {
                                if let Some(path) = path_arg {
                                    match std::fs::write(&path, &s) {
                                        Ok(()) => CommandResult::ok(format!(
                                            "Wrote config TOML to {path}"
                                        )),
                                        Err(e) => {
                                            CommandResult::err(format!("Failed to write file: {e}"))
                                        }
                                    }
                                } else {
                                    CommandResult::ok(s)
                                }
                            }
                            Err(e) => CommandResult::err(format!("TOML serialization failed: {e}")),
                        }
                    }
                    Err(e) => CommandResult::err(format!("Failed to load config: {e}")),
                },
                _ => unreachable!("invariant: outer match arm restricts subcommand to those listed above"),
            }
        }
        sub => CommandResult::err(format!("Unknown config subcommand: {sub}")),
    }
}

fn get_value(cfg: &crate::config::Config, key: &str) -> String {
    match key {
        "provider" | "default_provider" => cfg
            .default_provider
            .clone()
            .unwrap_or_else(|| "(not set)".to_string()),
        "model" | "default_model" => cfg
            .default_model
            .clone()
            .unwrap_or_else(|| "(not set)".to_string()),
        "temperature" | "default_temperature" => cfg.default_temperature.to_string(),
        "timeout" | "provider_timeout_secs" => cfg.provider_timeout_secs.to_string(),
        "workspace" | "workspace_dir" => cfg.workspace_dir.display().to_string(),
        "api_key" => cfg
            .api_key
            .as_ref()
            .map(|k| {
                if k.len() > 8 {
                    format!("{}...{}", &k[..4], &k[k.len() - 4..])
                } else {
                    "[set]".to_string()
                }
            })
            .unwrap_or_else(|| "(not set)".to_string()),
        "api_url" => cfg
            .api_url
            .clone()
            .unwrap_or_else(|| "(not set)".to_string()),
        "gateway.host" => cfg.gateway.host.clone(),
        "gateway.port" => cfg.gateway.port.to_string(),
        "memory.backend" => cfg.memory.backend.clone(),
        "memory.auto_save" => cfg.memory.auto_save.to_string(),
        "web_search.enabled" => cfg.web_search.enabled.to_string(),
        "web_search.provider" => cfg.web_search.provider.clone(),
        _ => format!("(unknown key: {key})"),
    }
}

fn set_value(cfg: &mut crate::config::Config, key: &str, val: &str) -> Result<(), String> {
    match key {
        "provider" | "default_provider" => cfg.default_provider = Some(val.to_string()),
        "model" | "default_model" => cfg.default_model = Some(val.to_string()),
        "temperature" | "default_temperature" => {
            cfg.default_temperature = val
                .parse::<f64>()
                .map_err(|_| format!("Invalid temperature: {val}"))?;
        }
        "timeout" | "provider_timeout_secs" => {
            cfg.provider_timeout_secs = val
                .parse::<u64>()
                .map_err(|_| format!("Invalid timeout: {val}"))?;
        }
        "api_key" => cfg.api_key = Some(val.to_string()),
        "api_url" => cfg.api_url = Some(val.to_string()),
        "gateway.host" => cfg.gateway.host = val.to_string(),
        "gateway.port" => {
            cfg.gateway.port = val
                .parse::<u16>()
                .map_err(|_| format!("Invalid port: {val}"))?;
        }
        "memory.backend" => cfg.memory.backend = val.to_string(),
        "memory.auto_save" => {
            cfg.memory.auto_save = val
                .parse::<bool>()
                .map_err(|_| format!("Invalid bool: {val}"))?;
        }
        "web_search.enabled" => {
            cfg.web_search.enabled = val
                .parse::<bool>()
                .map_err(|_| format!("Invalid bool: {val}"))?;
        }
        "web_search.provider" => cfg.web_search.provider = val.to_string(),
        "workspace" | "workspace_dir" => {
            return Err(
                "Workspace path is resolved at runtime; set SEN_WORKSPACE env var instead."
                    .to_string(),
            );
        }
        _ => return Err(format!("Unknown config key: {key}")),
    }
    Ok(())
}
