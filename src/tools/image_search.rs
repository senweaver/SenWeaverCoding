// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use super::web_search_tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct ImageSearchTool {
    max_results: usize,
    timeout_secs: u64,
}

impl ImageSearchTool {
    pub fn new(max_results: usize, timeout_secs: u64) -> Self {
        Self {
            max_results: max_results.clamp(1, 30),
            timeout_secs: timeout_secs.max(5),
        }
    }
}

#[async_trait]
impl Tool for ImageSearchTool {
    fn name(&self) -> &str {
        "image_search"
    }

    fn description(&self) -> &str {
        "Search for images on the web via the unified web_search DuckDuckGo Images engine. \
         Returns image URLs, thumbnails, source pages, and dimensions. Useful for finding \
         reference images, diagrams, logos."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 30},
                "size": {
                    "type": "string",
                    "enum": ["small", "medium", "large", "wallpaper"]
                },
                "image_type": {
                    "type": "string",
                    "enum": ["photo", "clipart", "gif", "transparent", "line"]
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
            obj.insert("engine".into(), json!("duckduckgo_images"));
            obj.insert("engine_only".into(), json!(true));
            obj.insert("multi".into(), json!(false));
            obj.insert("category".into(), json!("image"));
            obj.insert("max_results".into(), json!(max_results));
        }
        let inner = WebSearchTool::new_with_config(
            "duckduckgo_images".to_string(),
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
