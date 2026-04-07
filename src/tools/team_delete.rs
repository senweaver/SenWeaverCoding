// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::team_create::TeamRegistry;
use super::traits::{Tool, ToolResult};
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

#[cfg(test)]
mod tests {
    use super::super::team_create::{TeamInfo, TeamRegistry};
    use super::*;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn name_matches() {
        let registry: TeamRegistry = Arc::new(RwLock::new(HashMap::new()));
        assert_eq!(TeamDeleteTool::new(registry).name(), "team_delete");
    }

    #[tokio::test]
    async fn deletes_existing_team() {
        let registry: TeamRegistry = Arc::new(RwLock::new(HashMap::new()));
        registry.write().insert(
            "t-1".to_string(),
            TeamInfo {
                id: "t-1".to_string(),
                name: "test".to_string(),
                members: vec!["a".to_string()],
                leader: None,
                created_at: "2025-01-01".to_string(),
            },
        );
        let tool = TeamDeleteTool::new(Arc::clone(&registry));
        let result = tool.execute(json!({"team_id": "t-1"})).await.unwrap();
        assert!(result.success);
        assert!(registry.read().is_empty());
    }

    #[tokio::test]
    async fn returns_error_for_unknown_team() {
        let registry: TeamRegistry = Arc::new(RwLock::new(HashMap::new()));
        let tool = TeamDeleteTool::new(registry);
        let result = tool.execute(json!({"team_id": "nope"})).await.unwrap();
        assert!(!result.success);
    }
}
