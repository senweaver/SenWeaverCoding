// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct McpResourcesListTool;

impl McpResourcesListTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for McpResourcesListTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for McpResourcesListTool {
    fn name(&self) -> &str {
        "mcp_resources_list"
    }

    fn description(&self) -> &str {
        "List available resources from connected MCP servers. Returns resource URIs, names, descriptions, and MIME types."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Optional server name to filter resources from a specific MCP server"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let server_filter = args.get("server").and_then(|v| v.as_str());

        let svc = match crate::services::try_get_services() {
            Some(svc) => svc,
            None => {
                return Ok(ToolResult {
                    success: true,
                    output: "MCP services not initialized. Start the agent to connect MCP servers."
                        .to_string(),
                    error: None,
                });
            }
        };

        let servers = svc.mcp.list_servers().await;
        if servers.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: "No MCP servers connected. Configure MCP servers in your settings to see available resources."
                    .to_string(),
                error: None,
            });
        }

        let resources = svc.mcp.all_resources().await;
        let filtered: Vec<_> = if let Some(name) = server_filter {
            resources
                .into_iter()
                .filter(|r| r.server_name == name)
                .collect()
        } else {
            resources
        };

        if filtered.is_empty() {
            let msg = if let Some(name) = server_filter {
                format!(
                    "No resources found on MCP server '{name}'. \
                     The server may not expose resources, or resources haven't been discovered yet."
                )
            } else {
                "No resources found across connected MCP servers.".to_string()
            };
            return Ok(ToolResult {
                success: true,
                output: msg,
                error: None,
            });
        }

        let output = serde_json::to_string_pretty(&json!({
            "resources": filtered.iter().map(|r| {
                json!({
                    "uri": r.uri,
                    "name": r.name,
                    "description": r.description,
                    "mime_type": r.mime_type,
                    "server": r.server_name,
                })
            }).collect::<Vec<_>>(),
            "total": filtered.len(),
        }))?;

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
