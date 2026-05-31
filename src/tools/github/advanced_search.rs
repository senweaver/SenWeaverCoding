// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use super::super::web::search::tool::WebSearchTool;
use async_trait::async_trait;
use serde_json::json;

pub struct GitHubAdvancedSearchTool {
    token: Option<String>,
    timeout_secs: u64,
}

impl GitHubAdvancedSearchTool {
    pub fn new(token: Option<String>, timeout_secs: u64) -> Self {
        Self {
            token,
            timeout_secs: timeout_secs.max(5),
        }
    }

    pub fn from_env(timeout_secs: u64) -> Self {
        let token = std::env::var("GITHUB_TOKEN")
            .or_else(|_| std::env::var("GH_TOKEN"))
            .or_else(|_| std::env::var("SEN_GITHUB_TOKEN"))
            .ok();
        Self::new(token, timeout_secs)
    }
}

#[async_trait]
impl Tool for GitHubAdvancedSearchTool {
    fn name(&self) -> &str {
        "github_advanced_search"
    }

    fn description(&self) -> &str {
        "Advanced GitHub search exposing the full qualifier surface of https://github.com/search/advanced. \
         Combines positional keywords with structured filters: owners, repos, language, stars, forks, size, \
         created/pushed, topics, license, mirror/template/archived/fork visibility, in: title/body/comments, \
         path/extension/filename for code, state/labels/comments/author/assignee/mentions/team/commenter for issues & PRs, \
         draft/review/reviewed-by/review-requested for PRs. Thin wrapper over the unified web_search \
         github_advanced engine."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Free-form keywords to search for." },
                "search_type": {
                    "type": "string",
                    "enum": ["repositories", "code", "issues", "users", "commits"],
                    "description": "Endpoint to use (default: repositories)."
                },
                "sort": { "type": "string", "description": "Sort field for the chosen endpoint." },
                "order": { "type": "string", "enum": ["asc", "desc"], "description": "Sort order (default desc)." },
                "max_results": { "type": "integer", "minimum": 1, "maximum": 50 },
                "owners": { "type": "array", "items": { "type": "string" } },
                "repos": { "type": "array", "items": { "type": "string" } },
                "languages": { "type": "array", "items": { "type": "string" } },
                "topics": { "type": "array", "items": { "type": "string" } },
                "license": { "type": "string" },
                "stars": { "type": "string" },
                "forks": { "type": "string" },
                "size_kb": { "type": "string" },
                "followers": { "type": "string" },
                "created": { "type": "string" },
                "pushed": { "type": "string" },
                "updated": { "type": "string" },
                "merged": { "type": "string" },
                "closed": { "type": "string" },
                "archived": { "type": "boolean" },
                "is_mirror": { "type": "boolean" },
                "is_template": { "type": "boolean" },
                "is_fork": { "type": "string", "enum": ["only", "true", "false"] },
                "good_first_issues": { "type": "string" },
                "help_wanted_issues": { "type": "string" },
                "in_fields": { "type": "array", "items": { "type": "string" } },
                "filename": { "type": "string" },
                "extension": { "type": "string" },
                "path": { "type": "string" },
                "state": { "type": "string", "enum": ["open", "closed"] },
                "labels": { "type": "array", "items": { "type": "string" } },
                "milestone": { "type": "string" },
                "no_label": { "type": "boolean" },
                "no_milestone": { "type": "boolean" },
                "no_assignee": { "type": "boolean" },
                "linked": { "type": "string", "enum": ["pr", "issue"] },
                "type": { "type": "string", "enum": ["issue", "pr"] },
                "is_public": { "type": "boolean" },
                "is_private": { "type": "boolean" },
                "is_draft": { "type": "boolean" },
                "review": { "type": "string" },
                "reviewed_by": { "type": "string" },
                "review_requested": { "type": "string" },
                "team_review_requested": { "type": "string" },
                "author": { "type": "string" },
                "assignee": { "type": "string" },
                "mentions": { "type": "string" },
                "team": { "type": "string" },
                "commenter": { "type": "string" },
                "involves": { "type": "string" },
                "comments": { "type": "string" },
                "interactions": { "type": "string" },
                "reactions": { "type": "string" },
                "draft": { "type": "boolean" },
                "head": { "type": "string" },
                "base": { "type": "string" },
                "status": { "type": "string" },
                "language_in_user": { "type": "string" },
                "location": { "type": "string" }
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
            .unwrap_or(10)
            .clamp(1, 50);
        let mut delegated = args.clone();
        if let Some(obj) = delegated.as_object_mut() {
            obj.insert("engine".into(), json!("github_advanced"));
            obj.insert("engine_only".into(), json!(true));
            obj.insert("multi".into(), json!(false));
            obj.insert("category".into(), json!("code"));
            obj.insert("max_results".into(), json!(max_results));
        }
        let _ = &self.token;
        let inner = WebSearchTool::new_with_config(
            "github_advanced".to_string(),
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
