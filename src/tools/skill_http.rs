// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! HTTP-based tool derived from a skill's `[[tools]]` section.
//!
//! Each `SkillTool` with `kind = "http"` is converted into a `SkillHttpTool`
//! that implements the `Tool` trait. The command field is used as the URL
//! template and args are substituted as query parameters or path segments.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

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
        let url = self.substitute_args(&args);

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Only http:// and https:// URLs are allowed, got: {url}"
                )),
            });
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {e}"))?;

        let response = match client.get(&url).send().await {
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
        let body = match response.bytes().await {
            Ok(bytes) => {
                let mut text = String::from_utf8_lossy(&bytes).to_string();
                if text.len() > MAX_RESPONSE_BYTES {
                    let mut b = MAX_RESPONSE_BYTES.min(text.len());
                    while b > 0 && !text.is_char_boundary(b) {
                        b -= 1;
                    }
                    text.truncate(b);
                    text.push_str("\n... [response truncated at 1MB]");
                }
                text
            }
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read response body: {e}")),
                });
            }
        };

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
