// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Agent profile setup tool.
//!
//! Allows the agent to create new agent profiles (SOUL.md + config) from
//! within a conversation. Mirrors DeerFlow's `setup_agent` tool for
//! creating customized sub-agents with specific personalities and configurations.

use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct SetupAgentTool {
    security: Arc<SecurityPolicy>,
}

impl SetupAgentTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for SetupAgentTool {
    fn name(&self) -> &str {
        "setup_agent"
    }

    fn description(&self) -> &str {
        "Create or update an agent profile with a custom personality, system prompt, \
         and configuration. Creates a SOUL.md file and optional agent.toml config \
         under the agents directory."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Agent profile name (alphanumeric, hyphens, underscores; e.g. 'code-reviewer')"
                },
                "soul": {
                    "type": "string",
                    "description": "The SOUL.md content defining the agent's personality, expertise, and behavioral guidelines"
                },
                "description": {
                    "type": "string",
                    "description": "Brief description of the agent's purpose"
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for this agent (must be a model already added in Provider settings)"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "Optional system prompt override (if different from SOUL.md)"
                },
                "tool_groups": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional list of tool group names this agent should use (e.g. ['core', 'web'])"
                },
                "temperature": {
                    "type": "number",
                    "description": "Optional temperature override (0.0-2.0)"
                }
            },
            "required": ["name", "soul"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' parameter"))?;

        let soul = args
            .get("soul")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'soul' parameter"))?;

        if !is_valid_name(name) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Invalid agent name '{name}'. Use only alphanumeric characters, hyphens, and underscores."
                )),
            });
        }

        if soul.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("SOUL content must not be empty".into()),
            });
        }

        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let model = args.get("model").and_then(|v| v.as_str());
        let system_prompt = args.get("system_prompt").and_then(|v| v.as_str());
        let temperature = args.get("temperature").and_then(|v| v.as_f64());
        let tool_groups: Vec<String> = args
            .get("tool_groups")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let agent_dir = self.security.workspace_dir().join("agents").join(name);

        if let Err(e) = tokio::fs::create_dir_all(&agent_dir).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to create agent directory: {e}")),
            });
        }

        let soul_path = agent_dir.join("SOUL.md");
        if let Err(e) = tokio::fs::write(&soul_path, soul).await {
            let _ = tokio::fs::remove_dir_all(&agent_dir).await;
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write SOUL.md: {e}")),
            });
        }

        let has_config = model.is_some()
            || system_prompt.is_some()
            || temperature.is_some()
            || !tool_groups.is_empty()
            || !description.is_empty();

        if has_config {
            let mut toml_content = String::from("[agent]\n");

            if !description.is_empty() {
                toml_content.push_str(&format!("description = \"{}\"\n", escape_toml(description)));
            }
            if let Some(m) = model {
                toml_content.push_str(&format!("model = \"{m}\"\n"));
            }
            if let Some(sp) = system_prompt {
                toml_content.push_str(&format!("system_prompt = \"\"\"\n{}\n\"\"\"\n", sp));
            }
            if let Some(t) = temperature {
                toml_content.push_str(&format!("temperature = {t}\n"));
            }
            if !tool_groups.is_empty() {
                let groups_str: Vec<String> =
                    tool_groups.iter().map(|g| format!("\"{g}\"")).collect();
                toml_content.push_str(&format!("tool_groups = [{}]\n", groups_str.join(", ")));
            }

            let config_path = agent_dir.join("agent.toml");
            if let Err(e) = tokio::fs::write(&config_path, &toml_content).await {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to write agent.toml: {e}")),
                });
            }
        }

        let mut output = format!(
            "Agent profile '{}' created successfully.\n\nFiles:\n  - {}/SOUL.md",
            name,
            agent_dir.display()
        );
        if has_config {
            output.push_str(&format!("\n  - {}/agent.toml", agent_dir.display()));
        }
        output.push_str(&format!(
            "\n\nYou can now use this agent profile with the delegate tool: delegate agent=\"{name}\"",
        ));

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        && !name.starts_with('-')
        && !name.starts_with('_')
}

fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
