// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct BriefTool;

impl BriefTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BriefTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BriefTool {
    fn name(&self) -> &str {
        "send_user_message"
    }

    fn description(&self) -> &str {
        "Send a message to the user with markdown content and optional attachments. Use for proactive notifications or status updates."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Markdown content to send to the user"
                },
                "attachments": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "File paths to attach"
                },
                "status": {
                    "type": "string",
                    "description": "Message status type",
                    "enum": ["normal", "proactive"],
                    "default": "normal"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) if !c.trim().is_empty() => c.to_string(),
            Some(_) | None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing or empty required parameter: content".to_string()),
                });
            }
        };

        let attachments: Vec<String> = args
            .get("attachments")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let status = args
            .get("status")
            .and_then(|v| v.as_str())
            .filter(|s| *s == "normal" || *s == "proactive")
            .unwrap_or("normal");

        let output = json!({
            "content": content,
            "attachments": attachments,
            "status": status,
        })
        .to_string();

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_matches_tool_metadata() {
        let tool = BriefTool::new();
        let spec = tool.spec();
        assert_eq!(spec.name, "send_user_message");
        assert!(!spec.description.is_empty());
        assert_eq!(spec.parameters["type"], "object");
        assert!(spec.parameters["properties"]["content"].is_object());
        assert_eq!(spec.parameters["required"], json!(["content"]));
    }

    #[tokio::test]
    async fn execute_requires_content() {
        let tool = BriefTool::new();
        let r = tool.execute(json!({})).await.unwrap();
        assert!(!r.success);
        assert!(r.error.is_some());
    }

    #[tokio::test]
    async fn execute_returns_content_only() {
        let tool = BriefTool::new();
        let r = tool.execute(json!({ "content": "# Hello" })).await.unwrap();
        assert!(r.success);
        let v: serde_json::Value = serde_json::from_str(&r.output).unwrap();
        assert_eq!(v["content"], "# Hello");
        assert_eq!(v["attachments"], json!([]));
        assert_eq!(v["status"], "normal");
    }

    #[tokio::test]
    async fn execute_honors_attachments_and_status() {
        let tool = BriefTool::new();
        let r = tool
            .execute(json!({
                "content": "Done",
                "attachments": ["/tmp/a.txt", "/tmp/b.txt"],
                "status": "proactive"
            }))
            .await
            .unwrap();
        assert!(r.success);
        let v: serde_json::Value = serde_json::from_str(&r.output).unwrap();
        assert_eq!(v["content"], "Done");
        assert_eq!(v["attachments"], json!(["/tmp/a.txt", "/tmp/b.txt"]));
        assert_eq!(v["status"], "proactive");
    }
}
