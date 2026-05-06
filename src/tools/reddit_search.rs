// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Reddit search tool via public JSON API.
//!
//! Searches Reddit for posts across all subreddits or within a specific
//! subreddit using Reddit's public JSON API (no authentication required).
//! Inspired by Agent-Reach's Reddit search integration.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

pub struct RedditSearchTool {
    max_results: usize,
    timeout_secs: u64,
}

impl RedditSearchTool {
    pub fn new(max_results: usize, timeout_secs: u64) -> Self {
        Self {
            max_results: max_results.clamp(1, 25),
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
        "Search Reddit for posts and discussions. Returns titles, scores, comment counts, \
         and URLs. Useful for finding community discussions, opinions, troubleshooting \
         advice, and recommendations on any topic."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "subreddit": {
                    "type": "string",
                    "description": "Optional subreddit to search within (e.g. 'rust' for r/rust)"
                },
                "sort": {
                    "type": "string",
                    "enum": ["relevance", "hot", "top", "new", "comments"],
                    "description": "Sort order (default: relevance)"
                },
                "time": {
                    "type": "string",
                    "enum": ["all", "year", "month", "week", "day", "hour"],
                    "description": "Time filter (default: all)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results (default 10, max 25)"
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

        let subreddit = args.get("subreddit").and_then(|v| v.as_str());
        let sort = args
            .get("sort")
            .and_then(|v| v.as_str())
            .unwrap_or("relevance");
        let time = args.get("time").and_then(|v| v.as_str()).unwrap_or("all");
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.max_results as u64) as usize;
        let max = max.clamp(1, 25);

        let base = if let Some(sub) = subreddit {
            format!(
                "https://www.reddit.com/r/{}/search.json",
                sub.trim_start_matches("r/")
            )
        } else {
            "https://www.reddit.com/search.json".to_string()
        };

        let url = format!(
            "{}?q={}&sort={}&t={}&limit={}&restrict_sr={}",
            base,
            urlencoding::encode(query),
            sort,
            time,
            max,
            if subreddit.is_some() { "on" } else { "off" },
        );

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .user_agent("SenWeaverCoding/1.0 (compatible)")
            .build()?;

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Reddit request failed: {e}")),
                });
            }
        };

        if !resp.status().is_success() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Reddit API error: HTTP {}", resp.status())),
            });
        }

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to parse Reddit response: {e}")),
                });
            }
        };

        let posts = body
            .get("data")
            .and_then(|d| d.get("children"))
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();

        let context = if let Some(sub) = subreddit {
            format!("r/{}", sub.trim_start_matches("r/"))
        } else {
            "all of Reddit".to_string()
        };
        let mut output = format!(
            "Reddit search: \"{}\" in {} (sort: {}, time: {})\n\n",
            query, context, sort, time,
        );

        for (i, post) in posts.iter().take(max).enumerate() {
            let data = match post.get("data") {
                Some(d) => d,
                None => continue,
            };

            let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let sub = data.get("subreddit").and_then(|v| v.as_str()).unwrap_or("");
            let author = data.get("author").and_then(|v| v.as_str()).unwrap_or("");
            let score = data.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
            let comments = data
                .get("num_comments")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let permalink = data.get("permalink").and_then(|v| v.as_str()).unwrap_or("");
            let selftext = data.get("selftext").and_then(|v| v.as_str()).unwrap_or("");
            let created = data
                .get("created_utc")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let is_self = data
                .get("is_self")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let ext_url = data.get("url").and_then(|v| v.as_str()).unwrap_or("");

            let time_str = format_timestamp(created as i64);

            output.push_str(&format!(
                "{}. [r/{}] {} (score: {}, {} comments)\n   By u/{} | {}\n   https://www.reddit.com{}\n",
                i + 1,
                sub,
                title,
                score,
                comments,
                author,
                time_str,
                permalink,
            ));

            if !is_self && !ext_url.is_empty() && !ext_url.contains("reddit.com") {
                output.push_str(&format!("   Link: {ext_url}\n"));
            }

            if !selftext.is_empty() {
                let preview = if selftext.len() > 200 {
                    format!("{}...", &selftext[..200])
                } else {
                    selftext.to_string()
                };
                let cleaned = preview.replace('\n', " ");
                output.push_str(&format!("   {cleaned}\n"));
            }

            output.push('\n');
        }

        if posts.is_empty() {
            output.push_str("No results found.\n");
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

fn format_timestamp(unix: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - unix;

    if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 2_592_000 {
        format!("{}d ago", diff / 86400)
    } else if diff < 31_536_000 {
        format!("{}mo ago", diff / 2_592_000)
    } else {
        format!("{}y ago", diff / 31_536_000)
    }
}
