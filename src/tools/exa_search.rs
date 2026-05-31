// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::tools::web::search::tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct ExaSearchTool {
    api_key: Option<String>,
    max_results: usize,
    timeout_secs: u64,
}

impl ExaSearchTool {
    pub fn new(api_key: Option<String>, max_results: usize, timeout_secs: u64) -> Self {
        Self {
            api_key,
            max_results: max_results.clamp(1, 30),
            timeout_secs: timeout_secs.max(5),
        }
    }

    fn build_inner(&self) -> WebSearchTool {
        WebSearchTool::new_with_config(
            "exa".to_string(),
            None,
            None,
            self.max_results,
            self.timeout_secs,
            std::path::PathBuf::new(),
            false,
        )
        .with_extra_api_keys(None, self.api_key.clone())
    }
}

#[async_trait]
impl Tool for ExaSearchTool {
    fn name(&self) -> &str {
        "exa_search"
    }

    fn description(&self) -> &str {
        "Neural / semantic web search via Exa (formerly Metaphor). Thin wrapper around the unified \
         web_search Exa engine; requires EXA_API_KEY. Supports neural ranking, optional inline \
         content fetch, and category/domain filters."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "exa_type": {
                    "type": "string",
                    "enum": ["neural", "keyword", "auto"],
                    "default": "auto"
                },
                "max_results": {"type": "integer", "minimum": 1, "maximum": 30},
                "get_contents": {"type": "boolean", "default": false},
                "highlight_sentences": {"type": "integer", "minimum": 1, "maximum": 12},
                "category_filter": {"type": "string"},
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
            obj.insert("engine".into(), json!("exa"));
            obj.insert("engine_only".into(), json!(true));
            obj.insert("multi".into(), json!(false));
        }
        self.build_inner().execute(delegated).await
    }
}
