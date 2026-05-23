// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use super::web_search_tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct TavilySearchTool {
    api_key: Option<String>,
    max_results: usize,
    timeout_secs: u64,
}

impl TavilySearchTool {
    pub fn new(api_key: Option<String>, max_results: usize, timeout_secs: u64) -> Self {
        Self {
            api_key,
            max_results: max_results.clamp(1, 30),
            timeout_secs: timeout_secs.max(5),
        }
    }

    fn build_inner(&self) -> WebSearchTool {
        WebSearchTool::new_with_config(
            "tavily".to_string(),
            None,
            None,
            self.max_results,
            self.timeout_secs,
            std::path::PathBuf::new(),
            false,
        )
        .with_extra_api_keys(self.api_key.clone(), None)
    }
}

#[async_trait]
impl Tool for TavilySearchTool {
    fn name(&self) -> &str {
        "tavily_search"
    }

    fn description(&self) -> &str {
        "AI-optimised web search via Tavily API. Returns concise answers and ranked results \
         specifically designed for LLM consumption. Thin wrapper around the unified web_search \
         Tavily engine; requires TAVILY_API_KEY."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "search_depth": {
                    "type": "string",
                    "enum": ["basic", "advanced"],
                    "default": "basic"
                },
                "max_results": {"type": "integer", "minimum": 1, "maximum": 30},
                "include_answer": {"type": "boolean", "default": false},
                "include_raw_content": {"type": "boolean", "default": false},
                "include_domains": {"type": "array", "items": {"type": "string"}},
                "exclude_domains": {"type": "array", "items": {"type": "string"}}
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
        let mut delegated = args.clone();
        if let Some(obj) = delegated.as_object_mut() {
            obj.insert("engine".into(), json!("tavily"));
            obj.insert("engine_only".into(), json!(true));
            obj.insert("multi".into(), json!(false));
        }
        self.build_inner().execute(delegated).await
    }
}
