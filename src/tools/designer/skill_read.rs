// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::super::traits::{Tool, ToolResult};
use crate::agent::designer::skill;

const MAX_OUTPUT: usize = 200_000;

pub struct DesignerSkillReadTool;

impl DesignerSkillReadTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DesignerSkillReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for DesignerSkillReadTool {
    fn name(&self) -> &str {
        "designer_skill_read"
    }

    fn description(&self) -> &str {
        "Read a bundled Designer skill's side file on demand (Designer mode). Provide `id` (skill id, e.g. frontend-design, deck-swiss-international, taste-skill) and `path` (relative path such as assets/template.html, references/checklist.md). Call without `path` to list every available file for the given skill."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Skill id, e.g. frontend-design, deck-swiss-international, taste-skill."
                },
                "path": {
                    "type": "string",
                    "description": "Relative file path within the skill package. Omit to list available files."
                }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if id.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing required field `id`.".to_string()),
            });
        }
        if !skill::is_known(&id) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown skill id `{id}`.")),
            });
        }

        let path = args.get("path").and_then(|v| v.as_str()).map(str::trim);

        match path {
            None | Some("") => {
                let files = skill::list_files(&id);
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Available files for skill `{id}`:\n{}",
                        files
                            .iter()
                            .map(|p| format!("- {p}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                    error: None,
                })
            }
            Some(rel) => match skill::read_file(&id, rel) {
                Some(content) => {
                    let mut body = content.to_string();
                    let truncated = body.len() > MAX_OUTPUT;
                    if truncated {
                        crate::util::truncate_string_bytes(&mut body, MAX_OUTPUT);
                    }
                    let header = format!("# skill {id}/{rel}\n\n");
                    let suffix = if truncated {
                        "\n\n[truncated]".to_string()
                    } else {
                        String::new()
                    };
                    Ok(ToolResult {
                        success: true,
                        output: format!("{header}{body}{suffix}"),
                        error: None,
                    })
                }
                None => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "File `{rel}` not found in skill `{id}`. Call without `path` to list available files."
                    )),
                }),
            },
        }
    }
}
