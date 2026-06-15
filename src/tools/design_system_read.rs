// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::traits::{Tool, ToolResult};
use crate::agent::designer::design_system;

const MAX_OUTPUT: usize = 200_000;

pub struct DesignSystemReadTool;

impl DesignSystemReadTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DesignSystemReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for DesignSystemReadTool {
    fn name(&self) -> &str {
        "design_system_read"
    }

    fn description(&self) -> &str {
        "Read a bundled design-system pull-layer file on demand (Designer mode). Provide `id` (design system id) and `path` (relative path such as design-tokens.json, tailwind-v4.css, components.html, source/evidence.md). Call without `path` to list every available file for the given id."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Design system id, e.g. default, stripe, linear."
                },
                "path": {
                    "type": "string",
                    "description": "Relative file path within the design system package. Omit to list available files."
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
        if !design_system::is_known(&id) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown design system id `{id}`.")),
            });
        }

        let path = args.get("path").and_then(|v| v.as_str()).map(str::trim);

        match path {
            None | Some("") => {
                let files = design_system::list_files(&id);
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Available files for `{id}`:\n{}",
                        files
                            .iter()
                            .map(|p| format!("- {p}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                    error: None,
                })
            }
            Some(rel) => match design_system::read_file(&id, rel) {
                Some(content) => {
                    let mut body = content.to_string();
                    let truncated = body.len() > MAX_OUTPUT;
                    if truncated {
                        body.truncate(MAX_OUTPUT);
                    }
                    let header = format!("# {id}/{rel}\n\n");
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
                        "File `{rel}` not found in design system `{id}`. Call without `path` to list available files."
                    )),
                }),
            },
        }
    }
}
