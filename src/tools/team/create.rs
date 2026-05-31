// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    pub leader: Option<String>,
    pub created_at: String,
}

pub type TeamRegistry = Arc<RwLock<HashMap<String, TeamInfo>>>;

pub struct TeamCreateTool {
    registry: TeamRegistry,
}

impl TeamCreateTool {
    pub fn new(registry: TeamRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Create a new team of agents for coordinated multi-agent work. Teams have members, an optional leader, and shared task tracking."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name for the team"
                },
                "members": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of agent IDs to include as team members"
                },
                "leader": {
                    "type": "string",
                    "description": "Optional leader agent ID"
                }
            },
            "required": ["name", "members"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name'"))?;
        let members: Vec<String> = args
            .get("members")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let leader = args
            .get("leader")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if members.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Team must have at least one member".into()),
            });
        }

        let id = format!(
            "team-{}",
            uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("0")
        );
        let team = TeamInfo {
            id: id.clone(),
            name: name.to_string(),
            members: members.clone(),
            leader,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        self.registry.write().insert(id.clone(), team);

        Ok(ToolResult {
            success: true,
            output: json!({
                "team_id": id,
                "name": name,
                "member_count": members.len(),
            })
            .to_string(),
            error: None,
        })
    }
}
