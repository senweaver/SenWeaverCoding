// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use super::web_search_tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct GitHubSearchTool {
    token: Option<String>,
    timeout_secs: u64,
}

impl GitHubSearchTool {
    pub fn new(token: Option<String>, timeout_secs: u64) -> Self {
        Self {
            token,
            timeout_secs: timeout_secs.max(5),
        }
    }

    pub fn from_env(timeout_secs: u64) -> Self {
        let token = std::env::var("GITHUB_TOKEN")
            .or_else(|_| std::env::var("GH_TOKEN"))
            .or_else(|_| std::env::var("SEN_GITHUB_TOKEN"))
            .ok();
        Self::new(token, timeout_secs)
    }
}

#[async_trait]
impl Tool for GitHubSearchTool {
    fn name(&self) -> &str {
        "github_search"
    }

    fn description(&self) -> &str {
        "Search GitHub for repositories, code, issues, or users via the unified web_search \
         engines (github / github_code_search / github_issues / github_users). Returns structured \
         results with URLs, descriptions, stars, and other metadata."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "search_type": {
                    "type": "string",
                    "enum": ["repositories", "code", "issues", "users"],
                    "default": "repositories"
                },
                "sort": {"type": "string"},
                "order": {"type": "string", "enum": ["asc", "desc"]},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 30}
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").trim();
        if query.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("query parameter is required".into()),
            });
        }
        let search_type = args
            .get("search_type")
            .and_then(|v| v.as_str())
            .unwrap_or("repositories");
        let engine_id = match search_type {
            "code" => "github_code_search",
            "issues" => "github_issues",
            "users" => "github_users",
            _ => "github",
        };
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(10)
            .clamp(1, 30);
        let mut delegated = args.clone();
        if let Some(obj) = delegated.as_object_mut() {
            obj.insert("engine".into(), json!(engine_id));
            obj.insert("engine_only".into(), json!(true));
            obj.insert("multi".into(), json!(false));
            obj.insert("max_results".into(), json!(max_results));
            let category = if matches!(engine_id, "github_issues" | "github_users") {
                "forum"
            } else {
                "code"
            };
            obj.insert("category".into(), json!(category));
        }
        let _ = &self.token;
        let inner = WebSearchTool::new_with_config(
            engine_id.to_string(),
            None,
            None,
            max_results,
            self.timeout_secs,
            std::path::PathBuf::new(),
            false,
        );
        inner.execute(delegated).await
    }
}
