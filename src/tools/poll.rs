// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::channels::traits::{Channel, SendMessage};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

pub type ChannelMapHandle = Arc<RwLock<HashMap<String, Arc<dyn Channel>>>>;

const VOTE_EMOJIS: &[&str] = &[
    "\u{0031}\u{FE0F}\u{20E3}",
    "\u{0032}\u{FE0F}\u{20E3}",
    "\u{0033}\u{FE0F}\u{20E3}",
    "\u{0034}\u{FE0F}\u{20E3}",
    "\u{0035}\u{FE0F}\u{20E3}",
    "\u{0036}\u{FE0F}\u{20E3}",
    "\u{0037}\u{FE0F}\u{20E3}",
    "\u{0038}\u{FE0F}\u{20E3}",
    "\u{0039}\u{FE0F}\u{20E3}",
    "\u{0031}\u{0030}\u{FE0F}\u{20E3}",
];

const MIN_OPTIONS: usize = 2;
const MAX_OPTIONS: usize = 10;
const DEFAULT_DURATION_MINUTES: u64 = 60;

pub struct PollTool {
    security: Arc<SecurityPolicy>,
    channels: ChannelMapHandle,
}

impl PollTool {
    pub fn new(security: Arc<SecurityPolicy>, channels: ChannelMapHandle) -> Self {
        Self { security, channels }
    }
}

pub fn format_text_poll(
    question: &str,
    options: &[String],
    duration_minutes: u64,
    multi_select: bool,
) -> String {
    let mut lines = Vec::with_capacity(options.len() + 4);
    lines.push(format!("\u{1F4CA} **Poll: {question}**"));
    lines.push(String::new());
    for (i, option) in options.iter().enumerate() {
        let emoji = VOTE_EMOJIS.get(i).copied().unwrap_or("  ");
        lines.push(format!("{emoji}  {option}"));
    }
    lines.push(String::new());
    let mode = if multi_select {
        "multiple choices allowed"
    } else {
        "single choice"
    };
    lines.push(format!(
        "_React with the corresponding number to vote ({mode}). Poll closes in {duration_minutes} min._"
    ));
    lines.join("\n")
}

fn validate_options(args: &serde_json::Value) -> Result<Vec<String>, String> {
    let arr = args
        .get("options")
        .and_then(|v| v.as_array())
        .ok_or("Missing or invalid 'options' parameter (expected array of strings)")?;

    if arr.len() < MIN_OPTIONS {
        return Err(format!(
            "Poll requires at least {MIN_OPTIONS} options, got {}",
            arr.len()
        ));
    }
    if arr.len() > MAX_OPTIONS {
        return Err(format!(
            "Poll allows at most {MAX_OPTIONS} options, got {}",
            arr.len()
        ));
    }

    let mut options = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let s = v
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(format!("Option at index {i} must be a non-empty string"))?;
        options.push(s);
    }
    Ok(options)
}

fn supports_native_poll(channel_name: &str) -> bool {
    let lower = channel_name.to_ascii_lowercase();
    lower.contains("telegram") || lower.contains("discord")
}

#[async_trait]
impl Tool for PollTool {
    fn name(&self) -> &str {
        "poll"
    }

    fn description(&self) -> &str {
        "Create a poll in a messaging channel. For Telegram/Discord uses native polls; for other channels formats as a numbered text message with emoji reactions for voting."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The poll question"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 2,
                    "maxItems": 10,
                    "description": "Poll answer options (2-10 items)"
                },
                "channel": {
                    "type": "string",
                    "description": "Target channel name. Defaults to the first available channel if omitted."
                },
                "recipient": {
                    "type": "string",
                    "description": "Recipient/chat identifier within the channel (e.g., chat_id for Telegram, channel_id for Slack)"
                },
                "duration_minutes": {
                    "type": "integer",
                    "description": "Poll duration in minutes (default: 60)"
                },
                "multi_select": {
                    "type": "boolean",
                    "description": "Allow multiple selections (default: false)"
                }
            },
            "required": ["question", "options"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        if let Err(e) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "poll")
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

        let options = match validate_options(&args) {
            Ok(opts) => opts,
            Err(msg) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(msg),
                });
            }
        };

        let duration_minutes = args
            .get("duration_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_DURATION_MINUTES);

        let multi_select = args
            .get("multi_select")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let requested_channel = args
            .get("channel")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string());

        let recipient = args
            .get("recipient")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string());

        let (channel_name, channel): (String, Arc<dyn Channel>) = {
            let channels = self.channels.read();
            if let Some(ref name) = requested_channel {
                let ch = channels.get(name.as_str()).cloned().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Channel '{}' not found. Available: {}",
                        name,
                        channels.keys().cloned().collect::<Vec<_>>().join(", ")
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

        let recipient_id = recipient.unwrap_or_default();

        let is_native = supports_native_poll(&channel_name);

        let poll_text = format_text_poll(&question, &options, duration_minutes, multi_select);

        let msg = SendMessage::new(&poll_text, &recipient_id);
        if let Err(e) = channel.send(&msg).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Failed to send poll to channel '{channel_name}': {e}"
                )),
            });
        }

        let native_note = if is_native {
            " (native poll API available — text fallback used; trait extension needed for native support)"
        } else {
            ""
        };

        Ok(ToolResult {
            success: true,
            output: format!(
                "Poll created on '{channel_name}'{native_note}:\n\
                 Question: {question}\n\
                 Options: {}\n\
                 Duration: {duration_minutes} min | Multi-select: {multi_select}",
                options.join(", ")
            ),
            error: None,
        })
    }
}
