// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub use crate::services::team::store::{TeamInfo, TeamRegistry};

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
            leader: leader.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        self.registry.write().insert(id.clone(), team);

        let team_cfg = match crate::services::try_get_services() {
            Some(svc) => {
                let c = svc.config();
                crate::agent::team_protocol::TeamConfig {
                    message_channel_size: c.teams.message_channel_size.max(1),
                    max_team_size: c.teams.max_team_size.max(1),
                    ..crate::agent::team_protocol::TeamConfig::default()
                }
            }
            None => crate::agent::team_protocol::TeamConfig::default(),
        };
        let rejected = match crate::services::team::runtime::create_team(
            &id,
            name,
            &members,
            leader.as_deref(),
            team_cfg,
        ) {
            Ok(rejected) => rejected,
            Err(e) => {
                self.registry.write().remove(&id);
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("team runtime rejected creation: {e}")),
                });
            }
        };

        if let Some(svc) = crate::services::try_get_services() {
            svc.team_memory_sync
                .upsert(
                    &format!("team:{id}"),
                    &format!("Team '{name}' created with {} member(s)", members.len()),
                    "team_create",
                    vec!["team".to_string()],
                )
                .await;
        }

        Ok(ToolResult {
            success: true,
            output: json!({
                "team_id": id,
                "name": name,
                "member_count": members.len().saturating_sub(rejected.len()),
                "rejected_members": rejected,
            })
            .to_string(),
            error: None,
        })
    }
}
