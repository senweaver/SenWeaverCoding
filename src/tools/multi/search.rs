// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use super::super::web::search::tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct MultiSearchTool {
    max_results: usize,
    timeout_secs: u64,
    brave_api_key: Option<String>,
    searxng_url: Option<String>,
    tavily_api_key: Option<String>,
    exa_api_key: Option<String>,
}

impl MultiSearchTool {
    pub fn new(
        max_results: usize,
        timeout_secs: u64,
        brave_api_key: Option<String>,
        searxng_url: Option<String>,
    ) -> Self {
        Self {
            max_results: max_results.clamp(1, 30),
            timeout_secs: timeout_secs.max(5),
            brave_api_key,
            searxng_url,
            tavily_api_key: std::env::var("TAVILY_API_KEY").ok().filter(|s| !s.is_empty()),
            exa_api_key: std::env::var("EXA_API_KEY").ok().filter(|s| !s.is_empty()),
        }
    }

    pub fn with_tavily_key(mut self, key: Option<String>) -> Self {
        if key.is_some() {
            self.tavily_api_key = key;
        }
        self
    }

    pub fn with_exa_key(mut self, key: Option<String>) -> Self {
        if key.is_some() {
            self.exa_api_key = key;
        }
        self
    }

    fn build_inner(&self) -> WebSearchTool {
        WebSearchTool::new_with_config(
            String::new(),
            self.brave_api_key.clone(),
            self.searxng_url.clone(),
            self.max_results,
            self.timeout_secs,
            std::path::PathBuf::new(),
            false,
        )
        .with_extra_api_keys(self.tavily_api_key.clone(), self.exa_api_key.clone())
    }
}

#[async_trait]
impl Tool for MultiSearchTool {
    fn name(&self) -> &str {
        "multi_search"
    }

    fn description(&self) -> &str {
        "Search across multiple engines simultaneously (DuckDuckGo, Brave, SearXNG, Tavily, Exa, \
         Bing, Jina, Serper, plus locale-aware fallbacks) and return merged, de-duplicated, ranked \
         results. Thin wrapper over the unified web_search engine registry (multi-engine fan-out)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum total results after merging (default 10, max 30)"
                },
                "engines": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional engine id hints to prefer (e.g. duckduckgo, brave, searxng, tavily, exa, bing, jina). The chain still fans out to other engines for resilience."
                },
                "category": {
                    "type": "string",
                    "enum": ["web", "academic", "code", "cn", "news", "social", "video", "wiki", "lifestyle", "forum", "image"],
                    "description": "Optional category hint (default web)."
                },
                "time_range": {
                    "type": "string",
                    "enum": ["day", "week", "month", "year"]
                },
                "locale": {"type": "string"}
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if query.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Query must not be empty".into()),
            });
        }
        let mut delegated = args.clone();
        if let Some(obj) = delegated.as_object_mut() {
            obj.insert("multi".into(), json!(true));
            if let Some(engines) = obj.remove("engines") {
                if let Some(first_engine) = engines
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|v| v.as_str()))
                {
                    if !first_engine.trim().is_empty() {
                        obj.insert("engine".into(), json!(first_engine.trim()));
                    }
                }
            }
            obj.remove("engine_only");
        }
        self.build_inner().execute(delegated).await
    }
}
