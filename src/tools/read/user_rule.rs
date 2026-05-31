// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct ReadUserRuleTool;

impl ReadUserRuleTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReadUserRuleTool {
    fn name(&self) -> &str {
        "read_user_rule"
    }

    fn description(&self) -> &str {
        "Load the full contents of a user instruction rule by name. Use this when an entry in <available_user_rules> looks relevant to the current task and you need its complete body. The `name` parameter must match a `<name>` listed in that block (e.g. `coding.md` or `subdir/style.md`)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The rule name exactly as listed in <available_user_rules>."
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = args
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' parameter"))?;

        match crate::user_rules::read_user_rule(name) {
            Ok(content) => Ok(ToolResult {
                success: true,
                output: content,
                error: None,
            }),
            Err(err) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(err.to_string()),
            }),
        }
    }
}
