// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use super::github_common::{
    build_code_hit, build_issue_hit, build_repo_hit, build_user_hit, github_api_get, items_array,
};
use async_trait::async_trait;

pub struct GitHubAdvancedEngine;

#[async_trait]
impl SearchEngine for GitHubAdvancedEngine {
    fn id(&self) -> &'static str {
        "github_advanced"
    }

    fn label(&self) -> &'static str {
        "GitHub Advanced"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Code, SearchCategory::Forum]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let search_type = ctx
            .extra_str("search_type")
            .unwrap_or("repositories")
            .to_ascii_lowercase();
        let endpoint = match search_type.as_str() {
            "code" | "issues" | "users" | "commits" | "repositories" | "repos" => {
                if search_type == "repos" {
                    "repositories"
                } else {
                    search_type.as_str()
                }
            }
            _ => "repositories",
        };
        let accept = if endpoint == "code" {
            Some("application/vnd.github.v3.text-match+json")
        } else {
            None
        };
        let body = github_api_get(ctx, endpoint, accept).await?;
        let items = items_array(&body);
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let hit = match endpoint {
                "code" => build_code_hit(self.id(), item),
                "issues" => build_issue_hit(self.id(), item),
                "users" => build_user_hit(self.id(), item),
                "commits" => build_commit_hit(self.id(), item),
                _ => build_repo_hit(self.id(), item),
            };
            if let Some(h) = hit {
                hits.push(h);
            }
        }
        Ok(hits)
    }
}

fn build_commit_hit(engine_id: &'static str, item: &serde_json::Value) -> Option<SearchHit> {
    let sha = item.get("sha").and_then(|v| v.as_str())?;
    let url = item
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let message = item
        .get("commit")
        .and_then(|c| c.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .lines()
        .next()
        .map(clean_text)
        .unwrap_or_default();
    let author = item
        .get("commit")
        .and_then(|c| c.get("author"))
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let date = item
        .get("commit")
        .and_then(|c| c.get("author"))
        .and_then(|a| a.get("date"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let repo = item
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let short_sha: String = sha.chars().take(10).collect();
    let title = if message.is_empty() {
        format!("commit {short_sha}")
    } else {
        format!("[{short_sha}] {message}")
    };
    let source = match (author.is_empty(), repo.is_empty()) {
        (false, false) => format!("{author} — {repo} — GitHub Commits"),
        (false, true) => format!("{author} — GitHub Commits"),
        (true, false) => format!("{repo} — GitHub Commits"),
        _ => "GitHub Commits".to_string(),
    };
    let mut hit = SearchHit::new(engine_id, title, url).with_source(source);
    if let Some(d) = date {
        hit = hit.with_published_at(d);
    }
    Some(hit)
}
