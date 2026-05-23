// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "memory",
    aliases: &["mem"],
    description: "Manage persistent and session memories",
    usage: "/memory [list|add|remove|clear|search]",
    category: CommandCategory::Memory,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_memory),
});

pub async fn handle_memory(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let names = [
                "CLAUDE.md",
                "AGENTS.md",
                "MEMORY.md",
                ".senweavercoding/MEMORY.md",
                ".claude/CLAUDE.md",
            ];
            let mut found = Vec::new();
            for name in &names {
                let path = cwd.join(name);
                if path.exists() {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    found.push(format!("  {} ({} bytes)", name, size));
                }
            }
            if found.is_empty() {
                CommandResult::ok("No memory files found in workspace.")
            } else {
                let mut lines = vec!["Memory files:".to_string()];
                lines.extend(found);
                CommandResult::ok(lines.join("\n"))
            }
        }
        "add" => {
            if ctx.args.len() < 2 {
                return CommandResult::err("Usage: /memory add <content>");
            }
            let content = ctx.args[1..].join(" ");
            let cwd = std::env::current_dir().unwrap_or_default();
            let path = cwd.join("MEMORY.md");
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            let needs_newline = !existing.is_empty() && !existing.ends_with('\n');
            let updated = if needs_newline {
                format!("{}\n- {}", existing.trim_end(), content)
            } else {
                format!("{}- {}", existing, content)
            };
            match std::fs::write(&path, updated) {
                Ok(()) => CommandResult::ok(format!("Memory added to MEMORY.md: {}", content)),
                Err(e) => CommandResult::err(format!("Failed to write MEMORY.md: {e}")),
            }
        }
        "remove" | "delete" => {
            let key = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if key.is_empty() {
                return CommandResult::err(
                    "Usage: /memory remove <keyword>\n\
                    Removes all lines containing the keyword from MEMORY.md",
                );
            }
            let cwd = std::env::current_dir().unwrap_or_default();
            let path = cwd.join("MEMORY.md");
            if !path.exists() {
                return CommandResult::err("MEMORY.md not found. Nothing to remove.");
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => return CommandResult::err(format!("Failed to read MEMORY.md: {e}")),
            };
            let query_lower = key.to_lowercase();
            let original_lines = content.lines().count();
            let new_lines: Vec<&str> = content
                .lines()
                .filter(|line| !line.to_lowercase().contains(&query_lower))
                .collect();
            let removed_count = original_lines - new_lines.len();
            if removed_count == 0 {
                return CommandResult::ok(format!(
                    "No entries matching '{}' found in MEMORY.md.",
                    key
                ));
            }
            let new_content = new_lines.join("\n");
            match std::fs::write(&path, new_content) {
                Ok(()) => CommandResult::ok(format!(
                    "Removed {} line(s) matching '{}' from MEMORY.md.",
                    removed_count, key
                )),
                Err(e) => CommandResult::err(format!("Failed to update MEMORY.md: {e}")),
            }
        }
        "clear" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let session_mem = cwd.join(".senweavercoding/memory");
            if session_mem.exists() {
                match std::fs::remove_dir_all(&session_mem) {
                    Ok(()) => CommandResult::ok("Session memory cleared."),
                    Err(e) => CommandResult::err(format!("Failed to clear session memory: {e}")),
                }
            } else {
                CommandResult::ok("No session memory to clear.")
            }
        }
        "search" => {
            let query = ctx.args[1..].join(" ");
            if query.is_empty() {
                return CommandResult::err("Usage: /memory search <query>");
            }
            let cwd = std::env::current_dir().unwrap_or_default();
            let names = ["CLAUDE.md", "AGENTS.md", "MEMORY.md"];
            let mut matches = Vec::new();
            let query_lower = query.to_lowercase();
            for name in &names {
                let path = cwd.join(name);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for (i, line) in content.lines().enumerate() {
                        if line.to_lowercase().contains(&query_lower) {
                            matches.push(format!("  {}:{}: {}", name, i + 1, line.trim()));
                        }
                    }
                }
            }
            if matches.is_empty() {
                CommandResult::ok(format!("No matches found for: {}", query))
            } else {
                let mut lines = vec![format!("Search results for \"{}\":", query)];
                lines.extend(matches.into_iter().take(20));
                CommandResult::ok(lines.join("\n"))
            }
        }
        _ => CommandResult::err(format!(
            "Unknown memory subcommand: {subcmd}. Available: list, add, remove, clear, search"
        )),
    }
}
