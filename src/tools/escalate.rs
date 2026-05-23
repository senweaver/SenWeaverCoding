// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use crate::tools::ask_user::ChannelMapHandle;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

const PUSHOVER_API_URL: &str = "https://api.pushover.net/1/messages.json";
const PUSHOVER_REQUEST_TIMEOUT_SECS: u64 = 15;
const DEFAULT_TIMEOUT_SECS: u64 = 600;

const VALID_URGENCY_LEVELS: &[&str] = &["low", "medium", "high", "critical"];

pub struct EscalateToHumanTool {
    security: Arc<SecurityPolicy>,
    channel_map: ChannelMapHandle,
    workspace_dir: PathBuf,
}

impl EscalateToHumanTool {
    pub fn new(security: Arc<SecurityPolicy>, workspace_dir: PathBuf) -> Self {
        Self {
            security,
            channel_map: Arc::new(RwLock::new(HashMap::new())),
            workspace_dir,
        }
    }

    pub fn channel_map_handle(&self) -> ChannelMapHandle {
        Arc::clone(&self.channel_map)
    }

    fn format_message(urgency: &str, summary: &str, context: Option<&str>) -> String {
        let prefix = match urgency {
            "low" => "\u{2139}\u{fe0f} [LOW]",
            "high" => "\u{1f534} [HIGH]",
            "critical" => "\u{1f6a8} [CRITICAL]",

            _ => "\u{26a0}\u{fe0f} [MEDIUM]",
        };

        let mut lines = vec![
            format!("{prefix} Agent Escalation"),
            format!("Summary: {summary}"),
        ];

        if let Some(ctx) = context {
            lines.push(format!("Context: {ctx}"));
        }

        lines.push("---".to_string());
        lines.push("Reply to this message to respond.".to_string());

        lines.join("\n")
    }

    async fn get_pushover_credentials(&self) -> Option<(String, String)> {
        let env_path = self.workspace_dir.join(".env");
        let content = tokio::fs::read_to_string(&env_path).await.ok()?;

        let mut token = None;
        let mut user_key = None;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let line = line.strip_prefix("export ").map(str::trim).unwrap_or(line);
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = Self::parse_env_value(value);

                if key.eq_ignore_ascii_case("PUSHOVER_TOKEN") {
                    token = Some(value);
                } else if key.eq_ignore_ascii_case("PUSHOVER_USER_KEY") {
                    user_key = Some(value);
                }
            }
        }

        match (token, user_key) {
            (Some(t), Some(u)) if !t.is_empty() && !u.is_empty() => Some((t, u)),
            _ => None,
        }
    }

    fn parse_env_value(raw: &str) -> String {
        let raw = raw.trim();
        let unquoted = if raw.len() >= 2
            && ((raw.starts_with('"') && raw.ends_with('"'))
                || (raw.starts_with('\'') && raw.ends_with('\'')))
        {
            &raw[1..raw.len() - 1]
        } else {
            raw
        };
        unquoted.split_once(" #").map_or_else(
            || unquoted.trim().to_string(),
            |(value, _)| value.trim().to_string(),
        )
    }

    async fn send_pushover(&self, urgency: &str, summary: &str) {
        let creds = match self.get_pushover_credentials().await {
            Some(c) => c,
            None => {
                tracing::debug!(
                    "escalate_to_human: Pushover credentials not available, skipping push notification"
                );
                return;
            }
        };

        let priority = match urgency {
            "critical" => 1,
            "high" => 0,
            _ => return,
        };

        let form = reqwest::multipart::Form::new()
            .text("token", creds.0)
            .text("user", creds.1)
            .text("message", summary.to_string())
            .text("title", "Agent Escalation")
            .text("priority", priority.to_string());

        let client = crate::services::get_services()
            .proxy_runtime()
            .build_client_with_timeouts(
                "tool.escalate_to_human",
                PUSHOVER_REQUEST_TIMEOUT_SECS,
                10,
            );

        match client.post(PUSHOVER_API_URL).multipart(form).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("escalate_to_human: Pushover notification sent");
            }
            Ok(resp) => {
                tracing::warn!(
                    "escalate_to_human: Pushover returned status {}",
                    resp.status()
                );
            }
            Err(e) => {
                tracing::warn!("escalate_to_human: Pushover request failed: {e}");
            }
        }
    }
}

#[async_trait]
impl Tool for EscalateToHumanTool {
    fn name(&self) -> &str {
        "escalate_to_human"
    }

    fn description(&self) -> &str {
        "Escalate a situation to a human operator with urgency routing. \
         Sends a structured message to the active channel. High/critical urgency \
         also triggers a Pushover mobile notification when configured. \
         Optionally blocks to wait for a human response."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "One-line escalation summary"
                },
                "context": {
                    "type": "string",
                    "description": "Detailed context for the human"
                },
                "urgency": {
                    "type": "string",
                    "enum": ["low", "medium", "high", "critical"],
                    "description": "Urgency level (default: medium). high/critical triggers Pushover notification."
                },
                "wait_for_response": {
                    "type": "boolean",
                    "description": "Block and return the human's reply (default: false)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Seconds to wait for a response when wait_for_response is true (default: 600)"
                }
            },
            "required": ["summary"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        if let Err(e) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "escalate_to_human")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Action blocked: {e}")),
            });
        }

        let summary = args
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'summary' parameter"))?
            .to_string();

        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let urgency = args
            .get("urgency")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");

        if !VALID_URGENCY_LEVELS.contains(&urgency) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Invalid urgency '{}'. Must be one of: {}",
                    urgency,
                    VALID_URGENCY_LEVELS.join(", ")
                )),
            });
        }

        let wait_for_response = args
            .get("wait_for_response")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let timeout_secs = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        let text = Self::format_message(urgency, &summary, context.as_deref());

        let (channel_name, channel): (String, Arc<dyn Channel>) = {
            let channels = self.channel_map.read();
            if channels.is_empty() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("No channels available yet (channels not initialized)".to_string()),
                });
            }
            let (name, ch) = channels.iter().next().ok_or_else(|| {
                anyhow::anyhow!("No channels available. Configure at least one channel.")
            })?;
            (name.clone(), ch.clone())
        };

        let msg = SendMessage::new(&text, "");
        if let Err(e) = channel.send(&msg).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Failed to send escalation to channel '{channel_name}': {e}"
                )),
            });
        }

        if urgency == "high" || urgency == "critical" {
            self.send_pushover(urgency, &summary).await;
        }

        if wait_for_response {

            let (tx, mut rx) = tokio::sync::mpsc::channel::<ChannelMessage>(1);
            let timeout = std::time::Duration::from_secs(timeout_secs);

            let listen_channel = Arc::clone(&channel);
            let listen_handle =
                crate::runtime::spawn_supervised("tools.escalate.listen", async move {
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
        } else {

            Ok(ToolResult {
                success: true,
                output: json!({
                    "status": "escalated",
                    "urgency": urgency,
                    "channel": channel_name,
                })
                .to_string(),
                error: None,
            })
        }
    }
}
