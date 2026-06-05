// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::create::TeamRegistry;
use super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct TeamDeleteTool {
    registry: TeamRegistry,
}

impl TeamDeleteTool {
    pub fn new(registry: TeamRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        "team_delete"
    }

    fn description(&self) -> &str {
        "Delete a team and release its members. The team must be referenced by its ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "ID of the team to delete"
                }
            },
            "required": ["team_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let team_id = args
            .get("team_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'team_id'"))?;

        let removed = self.registry.write().remove(team_id);
        crate::services::team_runtime::delete_team(team_id);

        match removed {
            Some(team) => Ok(ToolResult {
                success: true,
                output: format!("Deleted team '{}' ({})", team.name, team_id),
                error: None,
            }),
            None => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Team '{}' not found", team_id)),
            }),
        }
    }
}
