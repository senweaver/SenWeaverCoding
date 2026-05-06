// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Interactive user prompting tool for cross-channel confirmations.
//!
//! Exposes `ask_user` as an agent-callable tool that sends a question to a
//! messaging channel and waits for the user's response. The tool holds a
//! late-binding channel map handle that is populated once channels are
//! initialized (after tool construction). This mirrors the pattern used by
//! [`ReactionTool`](super::reaction::ReactionTool).

use super::traits::{Tool, ToolResult};
use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

pub type ChannelMapHandle = Arc<RwLock<HashMap<String, Arc<dyn Channel>>>>;

const DEFAULT_TIMEOUT_SECS: u64 = 300;

pub struct AskUserTool {
    security: Arc<SecurityPolicy>,
    channels: ChannelMapHandle,
}

impl AskUserTool {

    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self {
            security,
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn channel_map_handle(&self) -> ChannelMapHandle {
        Arc::clone(&self.channels)
    }

    pub fn populate(&self, map: HashMap<String, Arc<dyn Channel>>) {
        *self.channels.write() = map;
    }
}

fn format_question(question: &str, choices: Option<&[String]>) -> String {
    let mut lines = Vec::new();
    lines.push(format!("**{question}**"));

    if let Some(choices) = choices {
        lines.push(String::new());
        for (i, choice) in choices.iter().enumerate() {
            lines.push(format!("{}. {choice}", i + 1));
        }
        lines.push(String::new());
        lines.push("_Reply with a number or type your answer._".to_string());
    }

    lines.join("\n")
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a question and wait for their response. \
         Sends the question to a messaging channel and blocks until the user replies \
         or the timeout expires. Optionally provide choices for structured responses."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "choices": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of choices (renders as buttons on Telegram, numbered list on CLI)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Seconds to wait for a response (default: 300)"
                },
                "channel": {
                    "type": "string",
                    "description": "Target channel name. Defaults to the first available channel if omitted."
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        if let Err(e) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "ask_user")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Action blocked: {e}")),
            });
        }

        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'question' parameter"))?
            .to_string();

        let choices: Option<Vec<String>> = args.get("choices").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
        });

        let timeout_secs = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        let requested_channel = args
            .get("channel")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string());

        let (channel_name, channel): (String, Arc<dyn Channel>) = {
            let channels = self.channels.read();
            if channels.is_empty() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("No channels available yet (channels not initialized)".to_string()),
                });
            }
            if let Some(ref name) = requested_channel {
                let ch = channels.get(name.as_str()).cloned().ok_or_else(|| {
                    let available: Vec<String> = channels.keys().cloned().collect();
                    anyhow::anyhow!(
                        "Channel '{}' not found. Available: {}",
                        name,
                        available.join(", ")
                    )
                })?;
                (name.clone(), ch)
            } else {
                let (name, ch) = channels.iter().next().ok_or_else(|| {
                    anyhow::anyhow!("No channels available. Configure at least one channel.")
                })?;
                (name.clone(), ch.clone())
            }
        };

        let text = format_question(&question, choices.as_deref());
        let msg = SendMessage::new(&text, "");
        if let Err(e) = channel.send(&msg).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Failed to send question to channel '{channel_name}': {e}"
                )),
            });
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel::<ChannelMessage>(1);
        let timeout = std::time::Duration::from_secs(timeout_secs);

        let listen_channel = Arc::clone(&channel);
        let listen_handle = crate::runtime::spawn_supervised("tools.ask_user.listen", async move {
            listen_channel.listen(tx).await
        });

        let response = tokio::time::timeout(timeout, rx.recv()).await;

        listen_handle.abort();

        match response {
            Ok(Some(msg)) => Ok(ToolResult {
                success: true,
                output: msg.content,
                error: None,
            }),
            Ok(None) => Ok(ToolResult {
                success: false,
                output: "TIMEOUT".to_string(),
                error: Some("Channel closed before receiving a response".to_string()),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: "TIMEOUT".to_string(),
                error: Some(format!(
                    "No response received within {timeout_secs} seconds"
                )),
            }),
        }
    }
}
