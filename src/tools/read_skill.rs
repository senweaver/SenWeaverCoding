// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

pub struct ReadSkillTool {
    workspace_dir: PathBuf,
    open_skills_enabled: bool,
    open_skills_dir: Option<String>,
    disabled_skills: Vec<String>,
}

impl ReadSkillTool {
    pub fn new(
        workspace_dir: PathBuf,
        open_skills_enabled: bool,
        open_skills_dir: Option<String>,
        disabled_skills: Vec<String>,
    ) -> Self {
        Self {
            workspace_dir,
            open_skills_enabled,
            open_skills_dir,
            disabled_skills,
        }
    }
}

#[async_trait]
impl Tool for ReadSkillTool {
    fn name(&self) -> &str {
        "read_skill"
    }

    fn description(&self) -> &str {
        "Read the full source file for an available skill by name. Use this in compact skills mode when you need the complete skill instructions without remembering file paths."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The skill name exactly as listed in <available_skills>."
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let requested = args
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' parameter"))?;

        let skills = crate::skills::filter_skills_by_disabled_list(
            crate::skills::load_skills_with_open_skills_settings(
                &self.workspace_dir,
                self.open_skills_enabled,
                self.open_skills_dir.as_deref(),
            ),
            &self.disabled_skills,
        );

        let Some(skill) = skills
            .iter()
            .find(|skill| skill.name.eq_ignore_ascii_case(requested))
        else {
            let mut names: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
            names.sort_unstable();
            let available = if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            };

            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown skill '{requested}'. Available skills: {available}"
                )),
            });
        };

        let Some(location) = skill.location.as_ref() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Skill '{}' has no readable source location.",
                    skill.name
                )),
            });
        };

        match tokio::fs::read_to_string(location).await {
            Ok(raw) => Ok(ToolResult {
                success: true,
                output: format_skill_payload(&skill.name, location, &raw),
                error: None,
            }),
            Err(err) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Failed to read skill '{}' from {}: {err}",
                    skill.name,
                    location.display()
                )),
            }),
        }
    }
}

fn format_skill_payload(name: &str, location: &std::path::Path, raw: &str) -> String {
    let normalized = raw.replace("\r\n", "\n");
    let body = if let Some(rest) = normalized.strip_prefix("---\n") {
        if let Some(idx) = rest.find("\n---\n") {
            rest[idx + 5..].to_string()
        } else if let Some(stripped) = rest.strip_suffix("\n---") {
            let _ = stripped;
            String::new()
        } else {
            normalized
        }
    } else {
        normalized
    };

    let mut out = String::new();
    out.push_str("# Skill: ");
    out.push_str(name);
    out.push('\n');
    out.push_str("Source: ");
    out.push_str(&location.display().to_string());
    out.push_str("\n\n");
    out.push_str(body.trim_start());
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}
