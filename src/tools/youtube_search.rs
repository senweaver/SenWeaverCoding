// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! YouTube video search tool.
//!
//! Searches YouTube for videos using the Invidious public API (no API key required)
//! or the official YouTube Data API v3 (requires API key).
//! Inspired by Agent-Reach's yt-dlp based YouTube search.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

pub struct YouTubeSearchTool {
    api_key: Option<String>,
    max_results: usize,
    timeout_secs: u64,
}

impl YouTubeSearchTool {
    pub fn new(api_key: Option<String>, max_results: usize, timeout_secs: u64) -> Self {
        Self {
            api_key,
            max_results: max_results.clamp(1, 20),
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
        "Search YouTube for videos. Returns video titles, URLs, channel names, \
         view counts, and descriptions. Useful for finding tutorials, demos, \
         talks, and other video content."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "YouTube search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results (default 5, max 20)"
                },
                "sort_by": {
                    "type": "string",
                    "enum": ["relevance", "date", "views", "rating"],
                    "description": "Sort order (default: relevance)"
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

        let sort_by = args
            .get("sort_by")
            .and_then(|v| v.as_str())
            .unwrap_or("relevance");

        if let Some(ref api_key) = self.api_key {
            self.search_official_api(query, max, sort_by, api_key).await
        } else {
            self.search_invidious(query, max, sort_by).await
        }
    }
}

impl YouTubeSearchTool {
    async fn search_official_api(
        &self,
        query: &str,
        max_results: usize,
        sort_by: &str,
        api_key: &str,
    ) -> anyhow::Result<ToolResult> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .build()?;

        let order = match sort_by {
            "date" => "date",
            "views" => "viewCount",
            "rating" => "rating",
            _ => "relevance",
        };

        let url = format!(
            "https://www.googleapis.com/youtube/v3/search?part=snippet&q={}&type=video&maxResults={}&order={}&key={}",
            urlencoding::encode(query),
            max_results,
            order,
            api_key,
        );

        let resp = client.get(&url).send().await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("YouTube API request failed: {e}")),
                });
            }
        };

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to parse YouTube API response: {e}")),
                });
            }
        };

        if let Some(error) = body.get("error") {
            let msg = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown API error");
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("YouTube API error: {msg}")),
            });
        }

        let items = body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut output = format!("YouTube results for: {query}\n\n");

        for (i, item) in items.iter().enumerate() {
            let snippet = item.get("snippet").unwrap_or(item);
            let video_id = item
                .get("id")
                .and_then(|id| id.get("videoId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let title = snippet.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let channel = snippet
                .get("channelTitle")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let description = snippet
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let published = snippet
                .get("publishedAt")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            output.push_str(&format!(
                "{}. {}\n   Channel: {} | Published: {}\n   https://www.youtube.com/watch?v={}\n",
                i + 1,
                title,
                channel,
                &published[..10.min(published.len())],
                video_id,
            ));
            if !description.is_empty() {
                let desc = if description.len() > 200 {
                    format!("{}...", &description[..200])
                } else {
                    description.to_string()
                };
                output.push_str(&format!("   {desc}\n"));
            }
            output.push('\n');
        }

        if items.is_empty() {
            output.push_str("No results found.\n");
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }

    async fn search_invidious(
        &self,
        query: &str,
        max_results: usize,
        sort_by: &str,
    ) -> anyhow::Result<ToolResult> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .user_agent("SenWeaverCoding/1.0")
            .build()?;

        let instances = [
            "https://vid.puffyan.us",
            "https://invidious.fdn.fr",
            "https://invidious.privacyredirect.com",
        ];

        let sort = match sort_by {
            "date" => "upload_date",
            "views" => "view_count",
            "rating" => "rating",
            _ => "relevance",
        };

        let mut last_error = String::new();

        for instance in &instances {
            let url = format!(
                "{}/api/v1/search?q={}&sort_by={}&type=video",
                instance,
                urlencoding::encode(query),
                sort,
            );

            let resp = match client.get(&url).send().await {
                Ok(r) if r.status().is_success() => r,
                Ok(r) => {
                    last_error = format!("{}: HTTP {}", instance, r.status());
                    continue;
                }
                Err(e) => {
                    last_error = format!("{}: {}", instance, e);
                    continue;
                }
            };

            let items: Vec<serde_json::Value> = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    last_error = format!("{}: parse error: {}", instance, e);
                    continue;
                }
            };

            let mut output = format!("YouTube results for: {query}\n\n");

            for (i, item) in items.iter().take(max_results).enumerate() {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let video_id = item.get("videoId").and_then(|v| v.as_str()).unwrap_or("");
                let author = item.get("author").and_then(|v| v.as_str()).unwrap_or("");
                let views = item.get("viewCount").and_then(|v| v.as_u64()).unwrap_or(0);
                let length = item
                    .get("lengthSeconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let published = item
                    .get("publishedText")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                output.push_str(&format!(
                    "{}. {}\n   Channel: {} | Views: {} | Duration: {} | {}\n   https://www.youtube.com/watch?v={}\n",
                    i + 1,
                    title,
                    author,
                    format_views(views),
                    format_duration(length),
                    published,
                    video_id,
                ));
                if !description.is_empty() {
                    let desc_short = if description.len() > 200 {
                        format!("{}...", &description[..200])
                    } else {
                        description.to_string()
                    };
                    output.push_str(&format!("   {desc_short}\n"));
                }
                output.push('\n');
            }

            if items.is_empty() {
                output.push_str("No results found.\n");
            }

            return Ok(ToolResult {
                success: true,
                output,
                error: None,
            });
        }

        Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!(
                "All Invidious instances failed. Last error: {last_error}"
            )),
        })
    }
}

fn format_views(views: u64) -> String {
    if views >= 1_000_000 {
        format!("{:.1}M", views as f64 / 1_000_000.0)
    } else if views >= 1_000 {
        format!("{:.1}K", views as f64 / 1_000.0)
    } else {
        format!("{views}")
    }
}

fn format_duration(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
