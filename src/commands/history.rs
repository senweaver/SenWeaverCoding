// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};
use std::path::PathBuf;

inventory::submit!(StaticSlashCommand {
    name: "history",
    aliases: &["hist"],
    description: "Manage conversation history",
    usage: "/history [list|clear|export]",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_history),
});

pub async fn handle_history(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("list");
    let cwd = std::env::current_dir().unwrap_or_default();
    let sessions_dir = cwd.join(".senweavercoding").join("sessions");

    match subcmd {
        "list" => match crate::cli::bg::list_sessions_sync(&cwd) {
            Ok(sessions) if sessions.is_empty() => CommandResult::ok("No recent sessions found."),
            Ok(sessions) => {
                let mut lines = vec!["Recent sessions:".to_string()];
                for s in sessions.iter().take(20) {
                    lines.push(format!(
                        "  {} | {} | {} | {}",
                        s.id,
                        s.status,
                        s.started_at,
                        s.cwd.display()
                    ));
                }
                CommandResult::ok(lines.join("\n"))
            }
            Err(e) => CommandResult::err(format!("Failed to list sessions: {e}")),
        },
        "clear" => match clear_history(&sessions_dir).await {
            Ok(count) => CommandResult::ok(format!("Cleared {} session(s) from history.", count)),
            Err(e) => CommandResult::err(format!("Failed to clear history: {e}")),
        },
        "export" => {
            let output_path = ctx.args.get(1).map(PathBuf::from);
            match export_history(&sessions_dir, output_path).await {
                Ok(path) => CommandResult::ok(format!("History exported to: {}", path.display())),
                Err(e) => CommandResult::err(format!("Failed to export history: {e}")),
            }
        }
        _ => CommandResult::err(format!(
            "Unknown history subcommand: {subcmd}. Available: list, clear, export"
        )),
    }
}

async fn clear_history(sessions_dir: &std::path::Path) -> anyhow::Result<usize> {
    if !sessions_dir.exists() {
        return Ok(0);
    }

    let mut count = 0usize;
    let mut entries = tokio::fs::read_dir(sessions_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".json"))
            {
                tokio::fs::remove_file(&path).await?;
                count += 1;
            }
        }
    }
    Ok(count)
}

async fn export_history(
    sessions_dir: &std::path::Path,
    output_path: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    if !sessions_dir.exists() {
        anyhow::bail!("No sessions directory found.");
    }

    let sessions = crate::cli::bg::list_sessions_sync(sessions_dir)?;
    if sessions.is_empty() {
        anyhow::bail!("No sessions to export.");
    }

    let path = output_path.unwrap_or_else(|| {
        let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        std::env::temp_dir().join(format!("sen_history_{}.json", ts))
    });

    let json = serde_json::to_string_pretty(&sessions)?;
    tokio::fs::write(&path, json).await?;
    Ok(path)
}
