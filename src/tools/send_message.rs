// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Mailbox for inter-agent messaging
pub type AgentMailbox = Arc<RwLock<HashMap<String, VecDeque<AgentMessage>>>>;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn mailbox_with_agents(ids: &[&str]) -> AgentMailbox {
        let mut m = HashMap::new();
        for id in ids {
            m.insert((*id).to_string(), VecDeque::new());
        }
        Arc::new(RwLock::new(m))
    }

    #[test]
    fn spec_matches_tool_metadata() {
        let mb = mailbox_with_agents(&[]);
        let tool = SendMessageTool::new(mb, "alice".into());
        let spec = tool.spec();
        assert_eq!(spec.name, "send_message");
        assert!(!spec.description.is_empty());
        assert_eq!(spec.parameters["required"], json!(["to", "content"]));
    }

    #[tokio::test]
    async fn delivers_to_target_mailbox() {
        let mb = mailbox_with_agents(&["bob"]);
        let tool = SendMessageTool::new(Arc::clone(&mb), "alice".into());
        let r = tool
            .execute(json!({ "to": "bob", "content": "hello" }))
            .await
            .unwrap();
        assert!(r.success);
        let guard = mb.read();
        let q = guard.get("bob").unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].from, "alice");
        assert_eq!(q[0].to, "bob");
        assert_eq!(q[0].content, "hello");
    }

    #[tokio::test]
    async fn broadcast_delivers_to_all_mailboxes() {
        let mb = mailbox_with_agents(&["bob", "carol"]);
        let tool = SendMessageTool::new(Arc::clone(&mb), "alice".into());
        let r = tool
            .execute(json!({ "to": "broadcast", "content": "all hands" }))
            .await
            .unwrap();
        assert!(r.success);
        let guard = mb.read();
        assert_eq!(guard["bob"].len(), 1);
        assert_eq!(guard["carol"].len(), 1);
        assert_eq!(guard["bob"][0].content, "all hands");
        assert_eq!(guard["carol"][0].content, "all hands");
        assert_eq!(guard["bob"][0].to, "broadcast");
    }

    #[tokio::test]
    async fn missing_to_fails() {
        let mb = mailbox_with_agents(&[]);
        let tool = SendMessageTool::new(mb, "alice".into());
        let r = tool.execute(json!({ "content": "x" })).await.unwrap();
        assert!(!r.success);
    }
}
