// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "skills",
    aliases: &["skill"],
    description: "Manage agent skills: list, create, edit, delete, search",
    usage: "/skills [list|create|edit|delete|search]",
    category: CommandCategory::Skills,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_skills),
});

pub async fn handle_skills(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let skills_dirs = [cwd.join(".senweavercoding/skills"), cwd.join("skills")];
            let mut found = Vec::new();
            for dir in &skills_dirs {
                if dir.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir()
                                || path.extension().map_or(false, |e| e == "md" || e == "yaml")
                            {
                                found.push(format!("  {}", path.display()));
                            }
                        }
                    }
                }
            }
            if found.is_empty() {
                CommandResult::ok(
                    "No skills found. Create a skill in .senweavercoding/skills/ or skills/.",
                )
            } else {
                let mut lines = vec!["Available skills:".to_string()];
                lines.extend(found);
                CommandResult::ok(lines.join("\n"))
            }
        }
        "create" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err("Usage: /skills create <name>");
            }
            let cwd = std::env::current_dir().unwrap_or_default();
            let skill_dir = cwd.join(format!(".senweavercoding/skills/{name}"));
            match std::fs::create_dir_all(&skill_dir) {
                Ok(()) => {
                    let skill_file = skill_dir.join("SKILL.md");
                    let content = format!(
                        "# {name}\n\nSkill description here.\n\n## Instructions\n\nSkill instructions here.\n"
                    );
                    let _ = std::fs::write(&skill_file, content);
                    CommandResult::ok(format!("Skill '{name}' created at {}", skill_dir.display()))
                }
                Err(e) => CommandResult::err(format!("Failed to create skill: {e}")),
            }
        }
        "edit" | "delete" => {
            let name = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err(format!("Usage: /skills {subcmd} <name>"));
            }
            if subcmd == "delete" {
                let cwd = std::env::current_dir().unwrap_or_default();
                let skill_dir = cwd.join(format!(".senweavercoding/skills/{name}"));
                if skill_dir.exists() {
                    match std::fs::remove_dir_all(&skill_dir) {
                        Ok(()) => CommandResult::ok(format!("Skill '{name}' deleted.")),
                        Err(e) => CommandResult::err(format!("Failed to delete skill: {e}")),
                    }
                } else {
                    CommandResult::err(format!("Skill '{name}' not found."))
                }
            } else {
                CommandResult::ok(format!(
                    "Open {name}/SKILL.md in your editor to edit the skill."
                ))
            }
        }
        "search" | "discover" => {
            let query = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
            if query.is_empty() {
                return CommandResult::err("Usage: /skills search <query>");
            }
            let forge_config = crate::skillforge::SkillForgeConfig {
                enabled: true,
                auto_integrate: false,
                ..Default::default()
            };
            let forge = crate::skillforge::SkillForge::new(forge_config);
            let report = match forge.forge().await {
                Ok(r) => r,
                Err(e) => return CommandResult::err(format!("Skill search failed: {e}")),
            };
            let query_lower = query.to_lowercase();
            let matching: Vec<_> = report
                .results
                .iter()
                .filter(|r| {
                    r.candidate.name.to_lowercase().contains(&query_lower)
                        || r.candidate
                            .description
                            .to_lowercase()
                            .contains(&query_lower)
                })
                .collect();
            if matching.is_empty() {
                CommandResult::ok(format!("No skills found matching '{query}'."))
            } else {
                let mut lines = vec![format!(
                    "Found {} skill(s) matching '{query}':",
                    matching.len()
                )];
                for r in matching.iter().take(10) {
                    lines.push(format!(
                        "  {}  -  {}",
                        r.candidate.name, r.candidate.description
                    ));
                    lines.push(format!("    {}", r.candidate.url));
                }
                CommandResult::ok(lines.join("\n"))
            }
        }
        _ => CommandResult::err(format!(
            "Unknown skills subcommand: {subcmd}. Use: list, create, edit, delete, search"
        )),
    }
}
