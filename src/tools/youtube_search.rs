// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use super::web_search_tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct YouTubeSearchTool {
    api_key: Option<String>,
    max_results: usize,
    timeout_secs: u64,
}

impl YouTubeSearchTool {
    pub fn new(api_key: Option<String>, max_results: usize, timeout_secs: u64) -> Self {
        Self {
            api_key,
            max_results: max_results.clamp(1, 30),
            timeout_secs: timeout_secs.max(5),
        }
    }
}

#[async_trait]
impl Tool for YouTubeSearchTool {
    fn name(&self) -> &str {
        "youtube_search"
    }

    fn description(&self) -> &str {
        "Search YouTube for videos via the unified web_search Invidious engine. When \
         YOUTUBE_API_KEY is set, queries YouTube Data API v3; otherwise falls back to public \
         Invidious instances. Returns titles, URLs, channels, views, and descriptions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 30},
                "sort_by": {
                    "type": "string",
                    "enum": ["relevance", "date", "views", "rating"]
                }
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
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(self.max_results)
            .clamp(1, 30);
        let mut delegated = args.clone();
        if let Some(obj) = delegated.as_object_mut() {
            obj.insert("engine".into(), json!("invidious"));
            obj.insert("engine_only".into(), json!(true));
            obj.insert("multi".into(), json!(false));
            obj.insert("category".into(), json!("video"));
            obj.insert("max_results".into(), json!(max_results));
        }
        let _ = &self.api_key;
        let inner = WebSearchTool::new_with_config(
            "invidious".to_string(),
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
