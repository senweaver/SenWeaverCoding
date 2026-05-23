// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "export",
    aliases: &[],
    description: "Export the current session transcript",
    usage: "/export [path] [--md|--markdown]",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_export),
});

pub async fn handle_export(ctx: CommandContext) -> CommandResult {
    let fmt = if ctx.args.iter().any(|a| a == "--md" || a == "--markdown") {
        "markdown"
    } else {
        "json"
    };

    let dest = ctx
        .args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            if fmt == "markdown" {
                format!("session_export_{ts}.md")
            } else {
                format!("session_export_{ts}.json")
            }
        });

    if let Some(bs) = crate::bootstrap::try_get_state() {
        let mut session_id = String::new();
        let mut cost = 0.0f64;
        let mut model_usage = std::collections::HashMap::<String, serde_json::Value>::new();
        bs.read(|state| {
            session_id = state.session_id.to_string();
            cost = state.total_cost_usd;
            for (model, usage) in &state.model_usage {
                model_usage.insert(
                    model.clone(),
                    serde_json::json!({
                        "input_tokens": usage.input_tokens,
                        "output_tokens": usage.output_tokens,
                        "cost_usd": usage.total_cost_usd,
                        "requests": usage.request_count,
                    }),
                );
            }
        });

        let history_path = std::env::current_dir()
            .unwrap_or_default()
            .join(format!(".senweavercoding/sessions/{session_id}.json"));

        let history_content: Vec<serde_json::Value> = if history_path.exists() {
            std::fs::read_to_string(&history_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let output = if fmt == "markdown" {
            let mut md = String::new();
            md.push_str(&format!("# Session Export: {session_id}\n\n"));
            md.push_str(&format!(
                "- **Date**: {}\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            ));
            md.push_str(&format!("- **Cost**: ${cost:.4}\n\n"));
            md.push_str("---\n\n");
            for msg in &history_content {
                let role = msg
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("unknown");
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                match role {
                    "user" => md.push_str(&format!("## User\n\n{content}\n\n")),
                    "assistant" => md.push_str(&format!("## Assistant\n\n{content}\n\n")),
                    "system" => md.push_str(&format!("> **System**: {content}\n\n")),
                    _ => md.push_str(&format!("### {role}\n\n{content}\n\n")),
                }
            }
            md
        } else {
            let export_data = serde_json::json!({
                "session_id": session_id,
                "cost_usd": cost,
                "model_usage": model_usage,
                "exported_at": chrono::Utc::now().to_rfc3339(),
                "messages": history_content,
            });
            serde_json::to_string_pretty(&export_data).unwrap_or_default()
        };

        match std::fs::write(&dest, &output) {
            Ok(()) => CommandResult::ok(format!(
                "Session exported to {dest} ({fmt}, {} bytes)",
                output.len()
            )),
            Err(e) => CommandResult::err(format!("Failed to write export file: {e}")),
        }
    } else {
        CommandResult::err("Cannot export: bootstrap state not initialized.")
    }
}
