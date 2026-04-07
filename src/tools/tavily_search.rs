// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

/// AI-optimised search via the Tavily Search API.
///
/// Tavily returns concise, LLM-friendly answers and ranked results with
/// relevance scores — ideal for agent workflows where token economy matters.
pub struct TavilySearchTool {
    api_key: Option<String>,
    max_results: usize,
    timeout_secs: u64,
}

impl TavilySearchTool {
    pub fn new(api_key: Option<String>, max_results: usize, timeout_secs: u64) -> Self {
        Self {
            api_key,
            max_results: max_results.clamp(1, 10),
            timeout_secs: timeout_secs.max(5),
        }
    }

    fn resolve_api_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| std::env::var("TAVILY_API_KEY").ok())
            .filter(|k| !k.trim().is_empty())
    }
}

#[derive(Debug, Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
    #[serde(default)]
    score: f64,
}

#[async_trait]
impl Tool for TavilySearchTool {
    fn name(&self) -> &str {
        "tavily_search"
    }

    fn description(&self) -> &str {
        "AI-optimised web search via Tavily API. Returns concise answers and ranked results \
         specifically designed for LLM consumption. Requires TAVILY_API_KEY."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "search_depth": {
                    "type": "string",
                    "description": "Search depth: 'basic' (fast) or 'advanced' (thorough)",
                    "enum": ["basic", "advanced"],
                    "default": "basic"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (1-10)",
                    "default": 5
                },
                "include_answer": {
                    "type": "boolean",
                    "description": "Include AI-generated answer summary",
                    "default": true
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let api_key = match self.resolve_api_key() {
            Some(k) => k,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "Tavily API key not configured. Set TAVILY_API_KEY environment variable \
                         or add tavily_api_key to [web_search] in config.toml"
                            .into(),
                    ),
                });
            }
        };

        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if query.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("query parameter is required".into()),
            });
        }

        let search_depth = args
            .get("search_depth")
            .and_then(|v| v.as_str())
            .unwrap_or("basic");
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(self.max_results)
            .clamp(1, 10);
        let include_answer = args
            .get("include_answer")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let body = json!({
            "api_key": api_key,
            "query": query,
            "search_depth": search_depth,
            "max_results": max_results,
            "include_answer": include_answer,
        });

        let client = reqwest::Client::new();
        let resp = client
            .post("https://api.tavily.com/search")
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(self.timeout_secs))
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                if !status.is_success() {
                    let text = r.text().await.unwrap_or_default();
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Tavily API error (HTTP {status}): {text}")),
                    });
                }

                match r.json::<TavilyResponse>().await {
                    Ok(data) => {
                        let mut output = String::new();
                        if let Some(ref answer) = data.answer {
                            output.push_str(&format!("## AI Answer\n\n{answer}\n\n"));
                        }

                        if !data.results.is_empty() {
                            output.push_str("## Results\n\n");
                            for (i, result) in data.results.iter().enumerate() {
                                output.push_str(&format!(
                                    "{}. **{}** (score: {:.2})\n   {}\n   {}\n\n",
                                    i + 1,
                                    result.title,
                                    result.score,
                                    result.url,
                                    result.content,
                                ));
                            }
                        }

                        if output.is_empty() {
                            output = "No results found.".to_string();
                        }

                        Ok(ToolResult {
                            success: true,
                            output,
                            error: None,
                        })
                    }
                    Err(e) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to parse Tavily response: {e}")),
                    }),
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Tavily request failed: {e}")),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid() {
        let tool = TavilySearchTool::new(None, 5, 15);
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
    }

    #[test]
    fn name_and_description() {
        let tool = TavilySearchTool::new(None, 5, 15);
        assert_eq!(tool.name(), "tavily_search");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn missing_api_key_returns_error() {
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::remove_var("TAVILY_API_KEY") };
        let tool = TavilySearchTool::new(None, 5, 15);
        let result = tool
            .execute(json!({"query": "rust programming"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("API key"));
    }

    #[tokio::test]
    async fn empty_query_returns_error() {
        let tool = TavilySearchTool::new(Some("tvly-test".into()), 5, 15);
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("required"));
    }
}
