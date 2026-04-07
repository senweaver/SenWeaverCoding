// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

/// Semantic search via the Exa API.
///
/// Exa uses neural/keyword search to find pages by meaning rather than just
/// keywords. Its `get_contents` option returns page text inline, reducing
/// the need for a separate `web_fetch` call.
pub struct ExaSearchTool {
    api_key: Option<String>,
    max_results: usize,
    timeout_secs: u64,
}

impl ExaSearchTool {
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
            .or_else(|| std::env::var("EXA_API_KEY").ok())
            .filter(|k| !k.trim().is_empty())
    }
}

#[derive(Debug, Deserialize)]
struct ExaResponse {
    #[serde(default)]
    results: Vec<ExaResult>,
}

#[derive(Debug, Deserialize)]
struct ExaResult {
    #[serde(default)]
    title: String,
    url: String,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default, rename = "publishedDate")]
    published_date: Option<String>,
}

#[async_trait]
impl Tool for ExaSearchTool {
    fn name(&self) -> &str {
        "exa_search"
    }

    fn description(&self) -> &str {
        "Semantic web search via Exa API. Finds pages by meaning using neural search, \
         ideal for code and technical queries. Can return page content inline. Requires EXA_API_KEY."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query (natural language works best for neural search)"
                },
                "search_type": {
                    "type": "string",
                    "description": "Search type: 'neural' (semantic) or 'keyword' (traditional)",
                    "enum": ["neural", "keyword"],
                    "default": "neural"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results (1-10)",
                    "default": 5
                },
                "get_contents": {
                    "type": "boolean",
                    "description": "Include page text in results (reduces need for web_fetch)",
                    "default": false
                },
                "category": {
                    "type": "string",
                    "description": "Filter by content category",
                    "enum": ["company", "research paper", "news", "github", "tweet", "movie", "song", "personal site", "pdf"]
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
                        "Exa API key not configured. Set EXA_API_KEY environment variable \
                         or add exa_api_key to [web_search] in config.toml"
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

        let search_type = args
            .get("search_type")
            .and_then(|v| v.as_str())
            .unwrap_or("neural");
        let num_results = args
            .get("num_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(self.max_results)
            .clamp(1, 10);
        let get_contents = args
            .get("get_contents")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut body = json!({
            "query": query,
            "type": search_type,
            "numResults": num_results,
        });

        if get_contents {
            body["contents"] = json!({
                "text": true,
            });
        }

        if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
            body["category"] = json!(category);
        }

        let client = reqwest::Client::new();
        let resp = client
            .post("https://api.exa.ai/search")
            .header("x-api-key", &api_key)
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
                        error: Some(format!("Exa API error (HTTP {status}): {text}")),
                    });
                }

                match r.json::<ExaResponse>().await {
                    Ok(data) => {
                        let mut output = String::new();
                        if data.results.is_empty() {
                            output = "No results found.".to_string();
                        } else {
                            output.push_str(&format!(
                                "## Exa Search Results ({} mode)\n\n",
                                search_type
                            ));
                            for (i, result) in data.results.iter().enumerate() {
                                output.push_str(&format!(
                                    "{}. **{}** (score: {:.2})\n   {}\n",
                                    i + 1,
                                    result.title,
                                    result.score,
                                    result.url,
                                ));
                                if let Some(ref author) = result.author {
                                    output.push_str(&format!("   Author: {author}\n"));
                                }
                                if let Some(ref date) = result.published_date {
                                    output.push_str(&format!("   Published: {date}\n"));
                                }
                                if let Some(ref text) = result.text {
                                    let truncated = if text.len() > 500 {
                                        format!("{}…", &text[..500])
                                    } else {
                                        text.clone()
                                    };
                                    output.push_str(&format!("   {truncated}\n"));
                                }
                                output.push('\n');
                            }
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
                        error: Some(format!("Failed to parse Exa response: {e}")),
                    }),
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Exa request failed: {e}")),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid() {
        let tool = ExaSearchTool::new(None, 5, 15);
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
    }

    #[test]
    fn name_and_description() {
        let tool = ExaSearchTool::new(None, 5, 15);
        assert_eq!(tool.name(), "exa_search");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn missing_api_key_returns_error() {
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::remove_var("EXA_API_KEY") };
        let tool = ExaSearchTool::new(None, 5, 15);
        let result = tool
            .execute(json!({"query": "rust async runtime"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("API key"));
    }

    #[tokio::test]
    async fn empty_query_returns_error() {
        let tool = ExaSearchTool::new(Some("exa-test".into()), 5, 15);
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("required"));
    }
}
