// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct GiteeEngine;

#[async_trait]
impl SearchEngine for GiteeEngine {
    fn id(&self) -> &'static str {
        "gitee"
    }

    fn label(&self) -> &'static str {
        "Gitee"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Code, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let per_page = ctx.limit.clamp(5, 50);
        let mut url = format!(
            "https://gitee.com/api/v5/search/repositories?q={encoded}&per_page={per_page}&sort=stars_count&order=desc"
        );
        if let Some(token) = ctx.api_keys.gitee_token.as_ref().filter(|s| !s.is_empty()) {
            url.push_str(&format!("&access_token={}", urlencoding::encode(token)));
        }
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Gitee search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let items = json.as_array().cloned().unwrap_or_default();
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let full_name = item
                .get("full_name")
                .or_else(|| item.get("human_name"))
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if full_name.is_empty() {
                continue;
            }
            let html_url = item
                .get("html_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if html_url.is_empty() {
                continue;
            }
            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let stars = item
                .get("stargazers_count")
                .or_else(|| item.get("stars_count"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let forks = item
                .get("forks_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let updated = item
                .get("updated_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let source_label = format!("Gitee 码云 · ⭐{stars} · 🔱 {forks}");
            let mut hit = SearchHit::new(self.id(), full_name, html_url)
                .with_description(description)
                .with_source(source_label);
            if let Some(t) = updated {
                hit = hit.with_published_at(t);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
