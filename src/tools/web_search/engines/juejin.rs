// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use serde_json::json;

pub struct JuejinEngine;

#[async_trait]
impl SearchEngine for JuejinEngine {
    fn id(&self) -> &'static str {
        "juejin"
    }

    fn label(&self) -> &'static str {
        "Juejin"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Cn, SearchCategory::Web]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let body = json!({
            "key_word": ctx.query,
            "page_no": 0,
            "page_size": ctx.limit.clamp(1, 30) as i64,
            "search_type": 0,
        });
        let client = ctx.build_http_client()?;
        let response = client
            .post("https://api.juejin.cn/search_api/v1/search")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Juejin search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let items = json
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let article = item.get("result_model").and_then(|v| v.get("article_info"));
            let Some(article) = article else {
                continue;
            };
            let title = article
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let article_id = article
                .get("article_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if article_id.is_empty() {
                continue;
            }
            let url = format!("https://juejin.cn/post/{article_id}");
            let description = article
                .get("brief_content")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            hits.push(SearchHit::new(self.id(), title, url).with_description(description));
        }
        Ok(hits)
    }
}
