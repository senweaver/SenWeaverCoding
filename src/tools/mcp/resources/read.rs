// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::sync::Arc;

use super::super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

use crate::services::mcp_manager::McpServerStatus;
use crate::tools::mcp::client::McpRegistry;

const MAX_RESOURCE_TEXT_BYTES: usize = 65_536;

pub struct McpResourcesReadTool {
    registry: Option<Arc<McpRegistry>>,
}

impl McpResourcesReadTool {
    pub fn new(registry: Option<Arc<McpRegistry>>) -> Self {
        Self { registry }
    }
}

impl Default for McpResourcesReadTool {
    fn default() -> Self {
        Self::new(None)
    }
}

#[async_trait]
impl Tool for McpResourcesReadTool {
    fn name(&self) -> &str {
        "mcp_resources_read"
    }

    fn description(&self) -> &str {
        "Read the content of a specific MCP resource by its URI. Requires the server name and resource URI."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Name of the MCP server that provides the resource"
                },
                "uri": {
                    "type": "string",
                    "description": "URI of the resource to read"
                }
            },
            "required": ["server", "uri"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let server = args
            .get("server")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'server' parameter"))?;
        let uri = args
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'uri' parameter"))?;

        let svc = match crate::services::try_get_services() {
            Some(svc) => svc,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "MCP services not initialized. Start the agent to connect MCP servers."
                            .to_string(),
                    ),
                });
            }
        };

        let server_info = match svc.mcp.get_server(server).await {
            Some(info) => info,
            None => {
                let available: Vec<String> = svc
                    .mcp
                    .list_servers()
                    .await
                    .into_iter()
                    .map(|s| s.name)
                    .collect();
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "MCP server '{}' not found. Available servers: {}",
                        server,
                        if available.is_empty() {
                            "(none)".to_string()
                        } else {
                            available.join(", ")
                        }
                    )),
                });
            }
        };

        if server_info.status != McpServerStatus::Connected {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "MCP server '{}' is not connected (status: {:?})",
                    server, server_info.status
                )),
            });
        }

        let meta = server_info.resources.iter().find(|r| r.uri == uri);

        let registry = self
            .registry
            .clone()
            .or_else(crate::tools::mcp::client::global_registry);

        if let Some(registry) = registry {
            match registry.read_resource(server, uri).await {
                Ok(contents) => {
                    let output = serde_json::to_string_pretty(&json!({
                        "uri": uri,
                        "name": meta.map(|m| m.name.clone()),
                        "server": server,
                        "contents": contents.iter().map(|c| {
                            json!({
                                "uri": c.uri,
                                "mimeType": c.mime_type,
                                "text": c.text.as_ref().map(|t| {
                                    crate::util::truncate_head_tail(t, MAX_RESOURCE_TEXT_BYTES, 70)
                                        .unwrap_or_else(|| t.clone())
                                }),
                                "blob": c.blob.as_ref().map(|b| {
                                    if b.len() > 200 {
                                        format!("{}... ({} chars, base64)", crate::util::truncate_str_bytes(b, 200), b.len())
                                    } else {
                                        b.clone()
                                    }
                                }),
                            })
                        }).collect::<Vec<_>>(),
                    }))
                    .unwrap_or_default();

                    Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    })
                }
                Err(e) => {
                    let known_uris: Vec<&str> = server_info
                        .resources
                        .iter()
                        .map(|r| r.uri.as_str())
                        .collect();
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Failed to read resource '{}' from server '{}': {e:#}. Known resources: {}",
                            uri,
                            server,
                            if known_uris.is_empty() {
                                "(none)".to_string()
                            } else {
                                known_uris.join(", ")
                            }
                        )),
                    })
                }
            }
        } else {
            Ok(ToolResult {
                success: false,
                output: serde_json::to_string_pretty(&json!({
                    "uri": uri,
                    "name": meta.map(|m| m.name.clone()),
                    "description": meta.and_then(|m| m.description.clone()),
                    "mime_type": meta.and_then(|m| m.mime_type.clone()),
                    "server": server,
                }))
                .unwrap_or_default(),
                error: Some(format!(
                    "Live content reading of resource '{}' on server '{}' \
                     requires an McpRegistry, which was not injected into this tool.",
                    uri, server
                )),
            })
        }
    }
}
