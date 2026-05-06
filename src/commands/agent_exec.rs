// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `/agent exec <agent_name> <prompt>` — run a standalone sub-agent
//! directly against the LLM, bypassing the main tool-call loop.
//!
//! Useful for:
//!   - Quick spot-checks of an agent's role/system prompt behaviour
//!   - Manual delegation without writing a workflow
//!   - Surfacing configuration issues (provider/model/system_prompt) early
//!
//! The command uses the same dispatch logic as `delegate_parallel`'s
//! per-task executor: look up `config.agents[agent_name]`, build a
//! provider, call `chat_with_system`, return the raw output.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "agent",
    aliases: &[],
    description: "Manage or directly invoke sub-agents (use: /agent exec <name> <prompt>)",
    usage: "/agent <list|exec <name> <prompt>>",
    category: CommandCategory::Tasks,
    hidden: false,
    requires_interactive: false,
    remote_safe: false,
    handler: make_handler!(handle_agent),
});

pub async fn handle_agent(ctx: CommandContext) -> CommandResult {
    let sub = ctx.args.first().map(String::as_str).unwrap_or("");
    match sub {
        "list" => list_configured_agents().await,
        "exec" => {
            let name = match ctx.args.get(1) {
                Some(n) if !n.is_empty() => n.clone(),
                _ => return CommandResult::err("Usage: /agent exec <agent_name> <prompt>"),
            };
            let prompt = if ctx.args.len() > 2 {
                ctx.args[2..].join(" ")
            } else {
                return CommandResult::err("Usage: /agent exec <agent_name> <prompt>");
            };
            exec_subagent(&name, &prompt).await
        }
        "" => CommandResult::err("Usage: /agent <list|exec <name> <prompt>>"),
        other => CommandResult::err(format!(
            "Unknown /agent subcommand '{other}'. Try: list | exec"
        )),
    }
}

async fn list_configured_agents() -> CommandResult {
    let cfg = match crate::config::Config::load_or_init().await {
        Ok(c) => c,
        Err(e) => return CommandResult::err(format!("Failed to load config: {e}")),
    };

    if cfg.agents.is_empty() {
        return CommandResult::ok(
            "No sub-agents configured.  Add entries under [agents.<name>] \
             in your config.toml.  See docs/multi-agent-tutorial.md."
                .to_string(),
        );
    }

    let mut lines = vec!["Configured sub-agents:".to_string()];
    for (name, a) in &cfg.agents {
        lines.push(format!(
            "  - {name:<20}  provider={}  model={}",
            a.provider, a.model
        ));
    }
    CommandResult::ok(lines.join("\n"))
}

async fn exec_subagent(name: &str, prompt: &str) -> CommandResult {
    let cfg = match crate::config::Config::load_or_init().await {
        Ok(c) => c,
        Err(e) => return CommandResult::err(format!("Failed to load config: {e}")),
    };

    let (provider_name, model, system_prompt, api_key, temperature) = match cfg.agents.get(name) {
        Some(a) => (
            a.provider.clone(),
            a.model.clone(),
            a.system_prompt.clone(),
            a.api_key.clone(),
            a.temperature.unwrap_or(0.7),
        ),
        None => {

            if name == "default" || name == "primary" {
                (
                    cfg.default_provider
                        .clone()
                        .unwrap_or_else(|| "openrouter".into()),
                    cfg.default_model
                        .clone()
                        .unwrap_or_else(|| "claude-sonnet-4-20250514".into()),
                    None,
                    cfg.api_key.clone(),
                    cfg.default_temperature,
                )
            } else {
                return CommandResult::err(format!(
                    "Sub-agent '{name}' not found in config. Run /agent list to see available agents."
                ));
            }
        }
    };

    let provider = match crate::providers::create_provider_with_url(
        &provider_name,
        api_key.as_deref(),
        None,
    ) {
        Ok(p) => p,
        Err(e) => {
            return CommandResult::err(format!("Failed to build provider '{provider_name}': {e}"));
        }
    };

    let start = std::time::Instant::now();
    match provider
        .chat_with_system(system_prompt.as_deref(), prompt, &model, temperature)
        .await
    {
        Ok(output) => {

            if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
                rt.blackboard.inner().write(
                    &format!(
                        "/agent_exec/{name}/{}",
                        chrono::Utc::now().timestamp_millis()
                    ),
                    serde_json::json!({
                        "agent": name,
                        "provider": &provider_name,
                        "model": &model,
                        "prompt_preview": prompt.chars().take(200).collect::<String>(),
                        "output_preview": output.chars().take(200).collect::<String>(),
                        "elapsed_ms": start.elapsed().as_millis() as u64,
                    }),
                    "agent_exec",
                    "invocations",
                );
            }

            CommandResult::ok(format!(
                "── sub-agent '{name}' output ({}ms, {}/{} provider/model) ──\n{output}",
                start.elapsed().as_millis(),
                provider_name,
                model
            ))
        }
        Err(e) => CommandResult::err(format!("Sub-agent '{name}' failed: {e}")),
    }
}
