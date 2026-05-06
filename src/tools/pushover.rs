// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const PUSHOVER_API_URL: &str = "https://api.pushover.net/1/messages.json";
const PUSHOVER_REQUEST_TIMEOUT_SECS: u64 = 15;

pub struct PushoverTool {
    security: Arc<SecurityPolicy>,
}

impl PushoverTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
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

    async fn get_credentials(&self) -> anyhow::Result<(String, String)> {
        let env_path = self.security.workspace_dir().join(".env");
        let content = tokio::fs::read_to_string(&env_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", env_path.display(), e))?;

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

        let token = token.ok_or_else(|| anyhow::anyhow!("PUSHOVER_TOKEN not found in .env"))?;
        let user_key =
            user_key.ok_or_else(|| anyhow::anyhow!("PUSHOVER_USER_KEY not found in .env"))?;

        Ok((token, user_key))
    }
}

#[async_trait]
impl Tool for PushoverTool {
    fn name(&self) -> &str {
        "pushover"
    }

    fn description(&self) -> &str {
        "Send a Pushover notification to your device. Requires PUSHOVER_TOKEN and PUSHOVER_USER_KEY in .env file."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The notification message to send"
                },
                "title": {
                    "type": "string",
                    "description": "Optional notification title"
                },
                "priority": {
                    "type": "integer",
                    "description": "Message priority: -2 (lowest/silent), -1 (low/no sound), 0 (normal), 1 (high), 2 (emergency/repeating)"
                },
                "sound": {
                    "type": "string",
                    "description": "Notification sound override (e.g., 'pushover', 'bike', 'bugle', 'cashregister', etc.)"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: rate limit exceeded".into()),
            });
        }

        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'message' parameter"))?
            .to_string();

        let title = args.get("title").and_then(|v| v.as_str()).map(String::from);

        let priority = match args.get("priority").and_then(|v| v.as_i64()) {
            Some(value) if (-2..=2).contains(&value) => Some(value),
            Some(value) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Invalid 'priority': {value}. Expected integer in range -2..=2"
                    )),
                });
            }
            None => None,
        };

        let sound = args.get("sound").and_then(|v| v.as_str()).map(String::from);

        let (token, user_key) = self.get_credentials().await?;

        let mut form = reqwest::multipart::Form::new()
            .text("token", token)
            .text("user", user_key)
            .text("message", message);

        if let Some(title) = title {
            form = form.text("title", title);
        }

        if let Some(priority) = priority {
            form = form.text("priority", priority.to_string());
        }

        if let Some(sound) = sound {
            form = form.text("sound", sound);
        }

        let client = crate::config::build_runtime_proxy_client_with_timeouts(
            "tool.pushover",
            PUSHOVER_REQUEST_TIMEOUT_SECS,
            10,
        );
        let response = client.post(PUSHOVER_API_URL).multipart(form).send().await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Ok(ToolResult {
                success: false,
                output: body,
                error: Some(format!("Pushover API returned status {}", status)),
            });
        }

        let api_status = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|json| json.get("status").and_then(|value| value.as_i64()));

        if api_status == Some(1) {
            Ok(ToolResult {
                success: true,
                output: format!(
                    "Pushover notification sent successfully. Response: {}",
                    body
                ),
                error: None,
            })
        } else {
            Ok(ToolResult {
                success: false,
                output: body,
                error: Some("Pushover API returned an application-level error".into()),
            })
        }
    }
}
