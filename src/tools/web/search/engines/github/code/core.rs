// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use async_trait::async_trait;

pub struct GitHubCodeEngine;

#[async_trait]
impl SearchEngine for GitHubCodeEngine {
    fn id(&self) -> &'static str {
        "github"
    }

    fn label(&self) -> &'static str {
        "GitHub"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Code]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let per_page = ctx.limit.clamp(1, 30);
        let url = format!(
            "https://api.github.com/search/repositories?q={encoded}&per_page={per_page}&sort=stars&order=desc"
        );
        let client = ctx.build_http_client()?;
        let mut req = client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = ctx.api_keys.github_token.as_deref() {
            if !token.is_empty() {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
        }
        let response = req.send().await?;
        if !response.status().is_success() {
            anyhow::bail!("GitHub search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let mut hits = Vec::new();
        if let Some(items) = json.get("items").and_then(|v| v.as_array()) {
            for item in items.iter().take(ctx.limit) {
                let full_name = item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = item
                    .get("html_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if full_name.is_empty() || url.is_empty() {
                    continue;
                }
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let stars = item
                    .get("stargazers_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let language = item
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let updated_at = item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut desc = String::new();
                if !language.is_empty() {
                    desc.push_str(&format!("[{language}] "));
                }
                desc.push_str(&format!("★{stars} "));
                if !description.is_empty() {
                    desc.push_str(&description);
                }
                let mut hit = SearchHit::new(self.id(), full_name, url)
                    .with_description(desc)
                    .with_source("GitHub");
                if !updated_at.is_empty() {
                    hit = hit.with_published_at(updated_at);
                }
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}
