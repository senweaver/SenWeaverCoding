// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct GitLabEngine;

#[async_trait]
impl SearchEngine for GitLabEngine {
    fn id(&self) -> &'static str {
        "gitlab"
    }

    fn label(&self) -> &'static str {
        "GitLab"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Code]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let per_page = ctx.limit.clamp(5, 30);
        let url = format!(
            "https://gitlab.com/api/v4/projects?search={encoded}&per_page={per_page}&order_by=star_count&sort=desc"
        );
        let client = ctx.build_http_client()?;
        let mut req = client.get(&url).header("Accept", "application/json");
        if let Some(token) = ctx.api_keys.gitlab_token.as_ref().filter(|s| !s.is_empty()) {
            req = req.header("PRIVATE-TOKEN", token.as_str());
        }
        let response = req.send().await?;
        if !response.status().is_success() {
            anyhow::bail!("GitLab search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let items = json.as_array().cloned().unwrap_or_default();
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let name = item
                .get("name_with_namespace")
                .or_else(|| item.get("path_with_namespace"))
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let web_url = item
                .get("web_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if web_url.is_empty() {
                continue;
            }
            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let stars = item.get("star_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let forks = item.get("forks_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let last_activity = item
                .get("last_activity_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let source_label = format!("GitLab · ⭐ {stars} · 🍴 {forks}");
            let mut hit = SearchHit::new(self.id(), name, web_url)
                .with_description(description)
                .with_source(source_label);
            if let Some(t) = last_activity {
                hit = hit.with_published_at(t);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
