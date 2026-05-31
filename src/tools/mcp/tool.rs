// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use async_trait::async_trait;

use crate::tools::mcp::client::McpRegistry;
use crate::tools::mcp::protocol::McpToolDef;
use crate::tools::traits::{Tool, ToolResult};

pub struct McpToolWrapper {

    prefixed_name: String,

    description: String,

    input_schema: serde_json::Value,

    registry: Arc<McpRegistry>,
}

impl McpToolWrapper {
    pub fn new(prefixed_name: String, def: McpToolDef, registry: Arc<McpRegistry>) -> Self {
        let description = def.description.unwrap_or_else(|| "MCP tool".to_string());
        Self {
            prefixed_name,
            description,
            input_schema: def.input_schema,
            registry,
        }
    }
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.prefixed_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        let args = match args {
            serde_json::Value::Object(mut map) => {
                map.remove("approved");
                serde_json::Value::Object(map)
            }
            other => other,
        };
        match self.registry.call_tool(&self.prefixed_name, args).await {
            Ok(output) => {
                let output = if crate::token_saver::is_enabled() {
                    crate::token_saver::compact_tool_output(
                        &format!("mcp_{}", self.prefixed_name),
                        &output,
                        &crate::token_saver::global(),
                    )
                } else {
                    output
                };
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}
