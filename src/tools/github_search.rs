// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! GitHub search tool via REST API.
//!
//! Searches GitHub repositories, code, issues, and users via the
//! public GitHub REST API. Supports authenticated requests (higher rate limits)
//! via GITHUB_TOKEN. Inspired by Agent-Reach's `gh` CLI integration.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

pub struct GitHubSearchTool {
    token: Option<String>,
    timeout_secs: u64,
}

impl GitHubSearchTool {
    pub fn new(token: Option<String>, timeout_secs: u64) -> Self {
        Self {
            token,
            timeout_secs: timeout_secs.max(5),
        }
    }

    pub fn from_env(timeout_secs: u64) -> Self {
        let token = std::env::var("GITHUB_TOKEN")
            .or_else(|_| std::env::var("GH_TOKEN"))
            .ok();
        Self::new(token, timeout_secs)
    }
}

#[async_trait]
impl Tool for GitHubSearchTool {
    fn name(&self) -> &str {
        "github_search"
    }

    fn description(&self) -> &str {
        "Search GitHub for repositories, code, issues, or users. Returns structured \
         results with URLs, descriptions, stars, and other metadata. Useful for finding \
         open-source projects, code examples, and developer resources."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (supports GitHub search qualifiers like 'language:rust stars:>100')"
                },
                "search_type": {
                    "type": "string",
                    "enum": ["repositories", "code", "issues", "users"],
                    "description": "Type of search (default: repositories)"
                },
                "sort": {
                    "type": "string",
                    "description": "Sort field: stars, forks, updated (repos); indexed (code); comments, created (issues); followers, repositories (users)"
                },
                "order": {
                    "type": "string",
                    "enum": ["asc", "desc"],
                    "description": "Sort order (default: desc)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results (default 10, max 30)"
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

        let search_type = args
            .get("search_type")
            .and_then(|v| v.as_str())
            .unwrap_or("repositories");

        let sort = args.get("sort").and_then(|v| v.as_str());
        let order = args.get("order").and_then(|v| v.as_str()).unwrap_or("desc");

        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;
        let max = max.clamp(1, 30);

        let mut url = format!(
            "https://api.github.com/search/{}?q={}&per_page={}&order={}",
            search_type,
            urlencoding::encode(query),
            max,
            order,
        );

        if let Some(s) = sort {
            url.push_str(&format!("&sort={s}"));
        }

        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .user_agent("SenWeaverCoding/1.0")
            .build()?
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json");

        if let Some(ref token) = self.token {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }

        let resp = match builder.send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("GitHub API request failed: {e}")),
                });
            }
        };

        if resp.status() == 403 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "GitHub API rate limit exceeded. Set GITHUB_TOKEN env var for higher limits."
                        .into(),
                ),
            });
        }

        if !resp.status().is_success() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("GitHub API error: HTTP {}", resp.status())),
            });
        }

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to parse GitHub response: {e}")),
                });
            }
        };

        let total = body
            .get("total_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let items = body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut output = format!(
            "GitHub {} search: {} ({} total results)\n\n",
            search_type, query, total
        );

        match search_type {
            "repositories" => format_repos(&items, &mut output),
            "code" => format_code(&items, &mut output),
            "issues" => format_issues(&items, &mut output),
            "users" => format_users(&items, &mut output),
            _ => format_repos(&items, &mut output),
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
}

fn format_repos(items: &[serde_json::Value], output: &mut String) {
    for (i, item) in items.iter().enumerate() {
        let name = item.get("full_name").and_then(|v| v.as_str()).unwrap_or("");
        let desc = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("(no description)");
        let stars = item
            .get("stargazers_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let forks = item
            .get("forks_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let lang = item.get("language").and_then(|v| v.as_str()).unwrap_or("?");
        let url = item.get("html_url").and_then(|v| v.as_str()).unwrap_or("");
        let updated = item
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        output.push_str(&format!(
            "{}. {} ({})\n   {} | Stars: {} | Forks: {} | Updated: {}\n   {}\n\n",
            i + 1,
            name,
            lang,
            url,
            stars,
            forks,
            &updated[..10.min(updated.len())],
            desc,
        ));
    }
}

fn format_code(items: &[serde_json::Value], output: &mut String) {
    for (i, item) in items.iter().enumerate() {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let path = item.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let repo = item
            .get("repository")
            .and_then(|r| r.get("full_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let url = item.get("html_url").and_then(|v| v.as_str()).unwrap_or("");

        output.push_str(&format!(
            "{}. {}/{}\n   Repo: {}\n   {}\n\n",
            i + 1,
            repo,
            path,
            repo,
            url,
        ));
        let _ = name;
    }
}

fn format_issues(items: &[serde_json::Value], output: &mut String) {
    for (i, item) in items.iter().enumerate() {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let state = item.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let url = item.get("html_url").and_then(|v| v.as_str()).unwrap_or("");
        let comments = item.get("comments").and_then(|v| v.as_u64()).unwrap_or(0);
        let user = item
            .get("user")
            .and_then(|u| u.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created = item
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let is_pr = url.contains("/pull/");
        let kind = if is_pr { "PR" } else { "Issue" };

        output.push_str(&format!(
            "{}. [{}] {} ({})\n   By: {} | Comments: {} | Created: {}\n   {}\n\n",
            i + 1,
            kind,
            title,
            state,
            user,
            comments,
            &created[..10.min(created.len())],
            url,
        ));
    }
}

fn format_users(items: &[serde_json::Value], output: &mut String) {
    for (i, item) in items.iter().enumerate() {
        let login = item.get("login").and_then(|v| v.as_str()).unwrap_or("");
        let url = item.get("html_url").and_then(|v| v.as_str()).unwrap_or("");
        let user_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("User");

        output.push_str(&format!(
            "{}. {} ({})\n   {}\n\n",
            i + 1,
            login,
            user_type,
            url,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = GitHubSearchTool::new(None, 10);
        assert_eq!(tool.name(), "github_search");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn empty_query_rejected() {
        let tool = GitHubSearchTool::new(None, 10);
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(!result.success);
    }
}
