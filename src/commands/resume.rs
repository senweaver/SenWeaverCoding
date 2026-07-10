// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "resume",
    aliases: &[],
    description: "Resume a previous session",
    usage: "/resume [session_id]",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_resume),
});

pub fn list_unified_sessions(workspace_root: &std::path::Path) -> Vec<(String, std::time::SystemTime)> {
    let sessions_root = workspace_root.join(".sen").join("sessions");
    let mut sessions: Vec<(String, std::time::SystemTime)> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&sessions_root) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let mut newest: Option<std::time::SystemTime> = None;
        if let Ok(files) = std::fs::read_dir(&path) {
            for file in files.flatten() {
                if let Ok(modified) = file.metadata().and_then(|m| m.modified()) {
                    if newest.is_none_or(|current| modified > current) {
                        newest = Some(modified);
                    }
                }
            }
        }
        let Some(modified) = newest else { continue };
        sessions.push((id, modified));
    }
    sessions.sort_by(|a, b| b.1.cmp(&a.1));
    sessions
}

pub fn unified_session_exists(workspace_root: &std::path::Path, id: &str) -> bool {
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return false;
    }
    workspace_root
        .join(".sen")
        .join("sessions")
        .join(id)
        .is_dir()
}

fn format_age(modified: std::time::SystemTime) -> String {
    match modified.elapsed() {
        Ok(age) => {
            let mins = age.as_secs() / 60;
            if mins < 60 {
                format!("{mins}m ago")
            } else if mins < 60 * 24 {
                format!("{}h {}m ago", mins / 60, mins % 60)
            } else {
                format!("{}d ago", mins / (60 * 24))
            }
        }
        Err(_) => "just now".to_string(),
    }
}

pub async fn handle_resume(ctx: CommandContext) -> CommandResult {
    let cwd = ctx.cwd.clone();

    match ctx.args.first().map(|s| s.as_str()) {
        Some("list") | None => {
            let sessions = list_unified_sessions(&cwd);
            if sessions.is_empty() {
                return CommandResult::ok(
                    "No previous sessions found. Start a new session first.",
                );
            }
            let mut lines = vec!["Available sessions (use /resume <id>):".to_string()];
            for (id, modified) in sessions.iter().take(10) {
                lines.push(format!("  {id}  ({})", format_age(*modified)));
            }
            CommandResult::ok(lines.join("\n"))
        }
        Some(id) => {
            if !unified_session_exists(&cwd, id) {
                return CommandResult::err(format!(
                    "Session '{id}' not found under .sen/sessions. Use /resume list to see available sessions.",
                ));
            }
            CommandResult::ok(format!(
                "Session '{id}' found. In the TUI, /resume {id} restores it directly; \
                 from the shell run `sen --continue` to resume the most recent session.",
            ))
        }
    }
}
