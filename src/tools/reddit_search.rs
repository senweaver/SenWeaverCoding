// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::tools::web::search::tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct RedditSearchTool {
    max_results: usize,
    timeout_secs: u64,
}

impl RedditSearchTool {
    pub fn new(max_results: usize, timeout_secs: u64) -> Self {
        Self {
            max_results: max_results.clamp(1, 30),
            timeout_secs: timeout_secs.max(5),
        }
    }
}

#[async_trait]
impl Tool for RedditSearchTool {
    fn name(&self) -> &str {
        "reddit_search"
    }

    fn description(&self) -> &str {
        "Search Reddit for posts and discussions via the unified web_search Reddit engine. \
         Supports subreddit / sort / time_filter filtering. Returns titles, scores, comments, \
         and URLs  -  useful for finding community discussions, opinions, troubleshooting advice."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "subreddit": {"type": "string"},
                "sort": {
                    "type": "string",
                    "enum": ["relevance", "hot", "top", "new", "comments"]
                },
                "time": {
                    "type": "string",
                    "enum": ["all", "year", "month", "week", "day", "hour"]
                },
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
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(self.max_results)
            .clamp(1, 30);
        let mut delegated = args.clone();
        if let Some(obj) = delegated.as_object_mut() {
            obj.insert("engine".into(), json!("reddit"));
            obj.insert("engine_only".into(), json!(true));
            obj.insert("multi".into(), json!(false));
            obj.insert("category".into(), json!("social"));
            obj.insert("max_results".into(), json!(max_results));
            if let Some(t) = obj.remove("time") {
                obj.insert("time_filter".into(), t);
            }
        }
        let inner = WebSearchTool::new_with_config(
            "reddit".to_string(),
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
