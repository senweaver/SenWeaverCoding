// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Web image search tool via DuckDuckGo.
//!
//! Searches for images across the web using DuckDuckGo's image search,
//! returning image URLs, thumbnails, titles, and source information.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct ImageSearchTool {
    max_results: usize,
    timeout_secs: u64,
}

impl ImageSearchTool {
    pub fn new(max_results: usize, timeout_secs: u64) -> Self {
        Self {
            max_results: max_results.clamp(1, 20),
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
        "Search for images on the web. Returns image URLs, thumbnails, titles, \
         and source pages. Useful for finding reference images, diagrams, logos, etc."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Image search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (default 5, max 20)"
                },
                "size": {
                    "type": "string",
                    "enum": ["small", "medium", "large", "wallpaper"],
                    "description": "Filter by image size"
                },
                "image_type": {
                    "type": "string",
                    "enum": ["photo", "clipart", "gif", "transparent", "line"],
                    "description": "Filter by image type"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        if query.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Query must not be empty".into()),
            });
        }

        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.max_results as u64) as usize;
        let max = max.clamp(1, 20);

        let size_filter = args.get("size").and_then(|v| v.as_str()).unwrap_or("");

        let type_filter = args
            .get("image_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut url = format!(
            "https://duckduckgo.com/i.js?q={}&o=json&p=1&s=0",
            urlencoding::encode(query)
        );

        if !size_filter.is_empty() {
            url.push_str(&format!("&iaf=size:{size_filter}"));
        }
        if !type_filter.is_empty() {
            url.push_str(&format!("&iaf=type:{type_filter}"));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .user_agent("Mozilla/5.0 (compatible; SenWeaverCoding/1.0)")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {e}"))?;

        let vqd = match get_vqd(&client, query).await {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to get search token: {e}")),
                });
            }
        };

        url.push_str(&format!("&vqd={vqd}"));

        let response = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Image search request failed: {e}")),
                });
            }
        };

        let body = match response.text().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read response: {e}")),
                });
            }
        };

        let parsed: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => {
                return Ok(ToolResult {
                    success: true,
                    output: format!("No image results found for: {query}"),
                    error: None,
                });
            }
        };

        let results = parsed
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if results.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No image results found for: {query}"),
                error: None,
            });
        }

        let mut output_items = Vec::new();
        for item in results.iter().take(max) {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let image_url = item.get("image").and_then(|v| v.as_str()).unwrap_or("");
            let thumbnail = item.get("thumbnail").and_then(|v| v.as_str()).unwrap_or("");
            let source = item.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let width = item.get("width").and_then(|v| v.as_u64()).unwrap_or(0);
            let height = item.get("height").and_then(|v| v.as_u64()).unwrap_or(0);

            output_items.push(json!({
                "title": title,
                "image_url": image_url,
                "thumbnail": thumbnail,
                "source": source,
                "width": width,
                "height": height,
            }));
        }

        let output = serde_json::to_string_pretty(&json!({
            "query": query,
            "total_results": output_items.len(),
            "results": output_items,
        }))
        .unwrap_or_default();

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

async fn get_vqd(client: &reqwest::Client, query: &str) -> anyhow::Result<String> {
    let url = format!("https://duckduckgo.com/?q={}", urlencoding::encode(query));
    let resp = client.get(&url).send().await?;
    let body = resp.text().await?;

    if let Some(pos) = body.find("vqd='") {
        let start = pos + 5;
        if let Some(end) = body[start..].find('\'') {
            return Ok(body[start..start + end].to_string());
        }
    }
    if let Some(pos) = body.find("vqd=\"") {
        let start = pos + 5;
        if let Some(end) = body[start..].find('"') {
            return Ok(body[start..start + end].to_string());
        }
    }
    if let Some(pos) = body.find("vqd=") {
        let start = pos + 4;
        let end = body[start..]
            .find(|c: char| !c.is_alphanumeric() && c != '-')
            .unwrap_or(body.len() - start);
        let token = &body[start..start + end];
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }

    anyhow::bail!("Could not extract vqd token from DuckDuckGo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = ImageSearchTool::new(5, 10);
        assert_eq!(tool.name(), "image_search");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn empty_query_rejected() {
        let tool = ImageSearchTool::new(5, 10);
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(!result.success);
    }
}
