// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::traits::{Tool, ToolResult};
use crate::agent::designer::html_template;

const MAX_OUTPUT: usize = 200_000;

pub struct DesignerTemplateReadTool;

impl DesignerTemplateReadTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DesignerTemplateReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for DesignerTemplateReadTool {
    fn name(&self) -> &str {
        "designer_template_read"
    }

    fn description(&self) -> &str {
        "Read a bundled Designer starting template's full HTML markup (Designer mode, From template). Provide `id` (template id, e.g. field-notes-editorial, executive-brief, web-prototype). Call without `id` to list every available built-in template with its title and category."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Built-in template id. Omit to list every available template."
                }
            }
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
            let listing = html_template::catalog()
                .iter()
                .map(|m| format!("- {} ({}) — {}", m.id, m.category, m.title))
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(ToolResult {
                success: true,
                output: format!("Available built-in templates:\n{listing}"),
                error: None,
            });
        }

        match html_template::read(&id) {
            Some(content) => {
                let mut body = content.to_string();
                let truncated = body.len() > MAX_OUTPUT;
                if truncated {
                    crate::util::truncate_string_bytes(&mut body, MAX_OUTPUT);
                }
                let header = format!("# template {id}/template.html\n\n");
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
                    "Unknown template id `{id}`. Call without `id` to list available templates."
                )),
            }),
        }
    }
}
