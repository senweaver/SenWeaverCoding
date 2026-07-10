// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "plugin",
    aliases: &["plugins"],
    description: "Manage plugins: list, enable, disable, install, remove",
    usage: "/plugin [list|enable|disable|install|remove]",
    category: CommandCategory::Tools,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_plugin),
});

#[cfg(feature = "plugins-wasm")]
async fn build_plugin_host() -> Result<crate::plugins::host::PluginHost, String> {
    let config = crate::config::Config::load_or_init()
        .await
        .map_err(|e| format!("Failed to load config: {e}"))?;
    if !config.plugins.enabled {
        return Err("Plugins are disabled in config (plugins.enabled = false).".to_string());
    }
    let plugins_dir = config.plugins.plugins_dir.clone();
    let plugin_path = if plugins_dir.starts_with("~/") {
        directories::UserDirs::new()
            .map(|u| u.home_dir().join(&plugins_dir[2..]))
            .unwrap_or_else(|| std::path::PathBuf::from(&plugins_dir))
    } else {
        std::path::PathBuf::from(&plugins_dir)
    };
    let workspace = plugin_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or(plugin_path);
    crate::plugins::host::PluginHost::from_plugins_config(&workspace, &config.plugins)
        .map_err(|e| format!("Failed to open plugin host: {e}"))
}

#[cfg(feature = "plugins-wasm")]
pub async fn handle_plugin(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            let host = match build_plugin_host().await {
                Ok(host) => host,
                Err(e) => return CommandResult::err(e),
            };
            let plugins = host.list_plugins();
            let disabled = host.disabled_plugin_names();
            if plugins.is_empty() && disabled.is_empty() {
                return CommandResult::ok(format!(
                    "No plugins installed in {}. Use /plugin install <path> to add one.",
                    host.plugins_dir().display()
                ));
            }
            let mut lines = vec![format!("Plugins in {}:", host.plugins_dir().display())];
            for p in &plugins {
                lines.push(format!(
                    "  {} v{} \u{2014} {} [enabled{}]",
                    p.name,
                    p.version,
                    p.description.as_deref().unwrap_or(""),
                    if p.loaded { "" } else { ", wasm missing" }
                ));
            }
            for name in &disabled {
                lines.push(format!("  {name} [disabled]"));
            }
            CommandResult::ok(lines.join("\n"))
        }
        "enable" | "disable" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err(format!("Usage: /plugin {subcmd} <name>"));
            }
            let mut host = match build_plugin_host().await {
                Ok(host) => host,
                Err(e) => return CommandResult::err(e),
            };
            let enable = subcmd == "enable";
            match host.set_enabled(name, enable) {
                Ok(()) => CommandResult::ok(format!(
                    "Plugin '{name}' {}. Restart the agent session for tool changes to take \
                     effect.",
                    if enable { "enabled" } else { "disabled" }
                )),
                Err(e) => CommandResult::err(format!("Failed to {subcmd} plugin '{name}': {e}")),
            }
        }
        "install" => {
            let path = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if path.is_empty() {
                return CommandResult::err(
                    "Usage: /plugin install <path>\n\
                    Example: /plugin install ./my-plugin",
                );
            }
            let mut host = match build_plugin_host().await {
                Ok(host) => host,
                Err(e) => return CommandResult::err(e),
            };
            match host.install(path) {
                Ok(()) => CommandResult::ok(format!(
                    "Plugin installed from: {path}\n\
                    Restart the agent session to load its tools."
                )),
                Err(e) => CommandResult::err(format!("Failed to install plugin: {e}")),
            }
        }
        "remove" | "uninstall" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err("Usage: /plugin remove <name>");
            }
            let mut host = match build_plugin_host().await {
                Ok(host) => host,
                Err(e) => return CommandResult::err(e),
            };
            match host.remove(name) {
                Ok(()) => CommandResult::ok(format!("Plugin '{name}' removed.")),
                Err(e) => CommandResult::err(format!("Failed to remove plugin '{name}': {e}")),
            }
        }
        _ => CommandResult::err(format!(
            "Unknown plugin subcommand: {subcmd}. Use: list, enable, disable, install, remove"
        )),
    }
}

#[cfg(not(feature = "plugins-wasm"))]
pub async fn handle_plugin(_ctx: CommandContext) -> CommandResult {
    CommandResult::err(
        "Plugin support is not compiled into this build (missing 'plugins-wasm' feature).",
    )
}
