// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};

use crate::tools::traits::Tool;

pub mod sse;
pub mod stdio;

pub const PROTOCOL_VERSION: &str = "2024-11-05";

pub const SERVER_NAME: &str = "senweavercoding";

pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct McpServer {

    tools: Arc<Vec<Arc<dyn Tool>>>,
}

impl McpServer {

    pub fn from_tools(candidates: Vec<Arc<dyn Tool>>) -> Self {
        let filtered: Vec<Arc<dyn Tool>> = candidates
            .into_iter()
            .filter(|t| t.mcp_safe())
            .collect();
        tracing::info!(
            target: "mcp.server",
            exposed = filtered.len(),
            "MCP server constructed; exposing mcp_safe tools"
        );
        Self {
            tools: Arc::new(filtered),
        }
    }

    pub fn exposed_tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn exposed_tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.tools.iter().map(|t| t.name().to_string()).collect();
        names.sort();
        names
    }

    pub async fn dispatch(&self, request: Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let is_notification = id.is_none();

        let outcome = match method {
            "initialize" => Ok(self.handle_initialize()),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(self.handle_tools_list()),
            "tools/call" => self.handle_tools_call(params).await,

            other => Err(McpError::method_not_found(other)),
        };

        if is_notification {
            return None;
        }

        let response = match outcome {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(err) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": err.to_value(),
            }),
        };
        Some(response)
    }

    fn handle_initialize(&self) -> Value {
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {

                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
            }
        })
    }

    fn handle_tools_list(&self) -> Value {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name(),
                    "description": t.description(),
                    "inputSchema": t.parameters_schema(),
                })
            })
            .collect();
        json!({ "tools": tools })
    }

    async fn handle_tools_call(&self, params: Value) -> Result<Value, McpError> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::invalid_params("missing 'name' field"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .cloned()
            .ok_or_else(|| {
                McpError::invalid_params(format!(
                    "tool '{name}' not exposed by this MCP server"
                ))
            })?;

        if let Err(reason) = tool.validate_args(&arguments) {
            return Ok(tool_error_payload(format!(
                "validate_args failed: {reason}"
            )));
        }
        if !tool.preflight_permission(&arguments) {
            return Ok(tool_error_payload(
                "preflight_permission denied".to_string(),
            ));
        }

        match crate::agent::loop_::execute_tool_panic_safe(tool.as_ref(), name, arguments).await {
            Ok(result) => Ok(json!({
                "content": [
                    { "type": "text", "text": result.output }
                ],
                "isError": !result.success,
            })),
            Err(e) => Ok(tool_error_payload(format!(
                "tool execution failed: {e}"
            ))),
        }
    }
}

fn tool_error_payload(message: String) -> Value {
    json!({
        "content": [
            { "type": "text", "text": message }
        ],
        "isError": true,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
}

impl McpError {
    pub fn method_not_found(name: &str) -> Self {
        Self {
            code: crate::tools::mcp::protocol::METHOD_NOT_FOUND,
            message: format!("method not found: {name}"),
        }
    }

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: crate::tools::mcp::protocol::INVALID_PARAMS,
            message: msg.into(),
        }
    }

    pub fn parse_error(msg: impl Into<String>) -> Self {
        Self {
            code: crate::tools::mcp::protocol::PARSE_ERROR,
            message: msg.into(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({ "code": self.code, "message": self.message })
    }
}
