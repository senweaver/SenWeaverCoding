// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use std::collections::HashMap;
const MAX_RESPONSE_BYTES: usize = 1_048_576;

const HTTP_TIMEOUT_SECS: u64 = 30;

pub struct SkillHttpTool {
    tool_name: String,
    tool_description: String,
    url_template: String,
    args: HashMap<String, String>,
}

impl SkillHttpTool {

    pub fn new(skill_name: &str, tool: &crate::skills::SkillTool) -> Self {
        Self {
            tool_name: format!("{}.{}", skill_name, tool.name),
            tool_description: tool.description.clone(),
            url_template: tool.command.clone(),
            args: tool.args.clone(),
        }
    }

    fn build_parameters_schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for (name, description) in &self.args {
            properties.insert(
                name.clone(),
                serde_json::json!({
                    "type": "string",
                    "description": description
                }),
            );
            required.push(serde_json::Value::String(name.clone()));
        }

        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }

    fn substitute_args(&self, args: &serde_json::Value) -> String {
        let mut url = self.url_template.clone();
        if let Some(obj) = args.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{{{}}}}}", key);
                let replacement = value.as_str().unwrap_or_default();
                url = url.replace(&placeholder, replacement);
            }
        }
        url
    }
}

#[async_trait]
impl Tool for SkillHttpTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.build_parameters_schema()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let raw_url = self.substitute_args(&args);

        let (allowed_domains, allow_private_hosts) = match crate::services::try_get_services() {
            Some(svc) => {
                let cfg = svc.config();
                (
                    cfg.http_request.allowed_domains.clone(),
                    cfg.http_request.allow_private_hosts,
                )
            }
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "skill http blocked: service container unavailable (fail-closed)".into(),
                    ),
                });
            }
        };

        let url = match crate::tools::http_request::validate_outbound_url(
            &raw_url,
            &allowed_domains,
            allow_private_hosts,
            false,
        ) {
            Ok(u) => u,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                });
            }
        };

        let client = crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("tool.skill_http", HTTP_TIMEOUT_SECS, 10);

        let mut response = match client.get(&url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("HTTP request failed: {e}")),
                });
            }
        };

        let status = response.status();

        let mut buf: Vec<u8> = Vec::new();
        let mut truncated = false;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if buf.len() + chunk.len() > MAX_RESPONSE_BYTES {
                        let take = MAX_RESPONSE_BYTES.saturating_sub(buf.len());
                        buf.extend_from_slice(&chunk[..take]);
                        truncated = true;
                        break;
                    }
                    buf.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to read response body: {e}")),
                    });
                }
            }
        }

        let mut body = String::from_utf8_lossy(&buf).to_string();
        if truncated {
            body.push_str("\n... [response truncated at 1MB]");
        }

        Ok(ToolResult {
            success: status.is_success(),
            output: body,
            error: if status.is_success() {
                None
            } else {
                Some(format!("HTTP {}", status))
            },
        })
    }
}
