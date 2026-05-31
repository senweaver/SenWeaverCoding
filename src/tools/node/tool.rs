// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::time::Duration;

use crate::gateway::nodes::{NodeInvocation, NodeRegistry};
use super::capabilities::requires_approval;
use crate::tools::traits::{Tool, ToolResult};

const NODE_INVOKE_TIMEOUT_SECS: u64 = 30;

pub struct NodeTool {

    prefixed_name: String,

    node_id: String,

    capability_name: String,

    description: String,

    parameters: serde_json::Value,

    registry: Arc<NodeRegistry>,
}

impl NodeTool {

    pub fn new(
        node_id: String,
        capability_name: String,
        description: String,
        parameters: serde_json::Value,
        registry: Arc<NodeRegistry>,
    ) -> Self {
        let prefixed_name = format!("node:{node_id}:{capability_name}");
        Self {
            prefixed_name,
            node_id,
            capability_name,
            description,
            parameters,
            registry,
        }
    }

    pub fn tool_name(node_id: &str, capability_name: &str) -> String {
        format!("node:{node_id}:{capability_name}")
    }
}

#[async_trait]
impl Tool for NodeTool {
    fn name(&self) -> &str {
        &self.prefixed_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        if requires_approval(&self.capability_name) {
            let approved = args
                .get("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !approved {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Capability '{}' requires approval. Set approved=true to proceed.",
                        self.capability_name
                    )),
                });
            }
        }

        let args = match args {
            serde_json::Value::Object(mut map) => {
                map.remove("approved");
                serde_json::Value::Object(map)
            }
            other => other,
        };

        let invoke_tx: tokio::sync::mpsc::Sender<NodeInvocation> =
            match self.registry.invoke_tx(&self.node_id) {
                Some(tx) => tx,
                None => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Node '{}' is not connected", self.node_id)),
                    });
                }
            };

        let call_id = uuid::Uuid::new_v4().to_string();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let invocation = NodeInvocation {
            call_id,
            capability: self.capability_name.clone(),
            args,
            response_tx,
        };

        if invoke_tx.send(invocation).await.is_err() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Failed to send invocation to node '{}'",
                    self.node_id
                )),
            });
        }

        match tokio::time::timeout(Duration::from_secs(NODE_INVOKE_TIMEOUT_SECS), response_rx).await
        {
            Ok(Ok(result)) => Ok(ToolResult {
                success: result.success,
                output: result.output,
                error: result.error,
            }),
            Ok(Err(_)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Node '{}' dropped the invocation channel",
                    self.node_id
                )),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Node '{}' invocation timed out after {NODE_INVOKE_TIMEOUT_SECS}s",
                    self.node_id
                )),
            }),
        }
    }
}
