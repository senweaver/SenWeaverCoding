// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub type AgentMailbox = Arc<RwLock<HashMap<String, VecDeque<AgentMessage>>>>;

pub fn global_mailbox() -> AgentMailbox {
    static GLOBAL: std::sync::OnceLock<AgentMailbox> = std::sync::OnceLock::new();
    GLOBAL
        .get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
        .clone()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentMessage {
    pub from: String,
    pub to: String,
    pub content: String,
    pub timestamp: String,
}

pub struct SendMessageTool {
    mailbox: AgentMailbox,
    sender_id: String,
}

impl SendMessageTool {
    pub fn new(mailbox: AgentMailbox, sender_id: String) -> Self {
        Self { mailbox, sender_id }
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to a teammate agent. Messages are delivered to the agent's mailbox for asynchronous processing."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Target agent ID or \"broadcast\" for all"
                },
                "content": {
                    "type": "string",
                    "description": "Message content"
                }
            },
            "required": ["to", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let to = match args.get("to").and_then(|v| v.as_str()) {
            Some(t) if !t.trim().is_empty() => t.trim().to_string(),
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing or empty required parameter: to".to_string()),
                });
            }
        };

        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) if !c.trim().is_empty() => c.to_string(),
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing or empty required parameter: content".to_string()),
                });
            }
        };

        let timestamp = chrono::Utc::now().to_rfc3339();
        let msg = AgentMessage {
            from: self.sender_id.clone(),
            to: to.clone(),
            content,
            timestamp: timestamp.clone(),
        };

        let mut guard = self.mailbox.write();
        if to.eq_ignore_ascii_case("broadcast") {
            for queue in guard.values_mut() {
                queue.push_back(msg.clone());
            }
        } else {
            guard.entry(to).or_default().push_back(msg);
        }

        Ok(ToolResult {
            success: true,
            output: json!({ "delivered": true, "timestamp": timestamp }).to_string(),
            error: None,
        })
    }
}

pub struct ReadMessagesTool {
    mailbox: AgentMailbox,
    agent_id: String,
}

impl ReadMessagesTool {
    pub fn new(mailbox: AgentMailbox, agent_id: String) -> Self {
        Self { mailbox, agent_id }
    }
}

#[async_trait]
impl Tool for ReadMessagesTool {
    fn name(&self) -> &str {
        "read_messages"
    }

    fn description(&self) -> &str {
        "Read and drain pending messages addressed to this agent's mailbox (delivered by other agents via send_message)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "peek": {
                    "type": "boolean",
                    "description": "If true, return messages without removing them from the mailbox (default false)."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let peek = args.get("peek").and_then(|v| v.as_bool()).unwrap_or(false);
        let messages: Vec<AgentMessage> = {
            let mut guard = self.mailbox.write();
            match guard.get_mut(&self.agent_id) {
                Some(queue) if peek => queue.iter().cloned().collect(),
                Some(queue) => queue.drain(..).collect(),
                None => Vec::new(),
            }
        };

        Ok(ToolResult {
            success: true,
            output: json!({
                "agent_id": self.agent_id,
                "count": messages.len(),
                "messages": messages,
            })
            .to_string(),
            error: None,
        })
    }
}
