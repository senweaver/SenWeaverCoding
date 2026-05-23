// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct SspaiEngine;

#[async_trait]
impl SearchEngine for SspaiEngine {
    fn id(&self) -> &'static str {
        "sspai"
    }

    fn label(&self) -> &'static str {
        "少数派"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Lifestyle, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(5, 30);
        let url = format!(
            "https://sspai.com/api/v1/search/article/page/get?limit={limit}&offset=0&keyword={encoded}"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("sspai search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let data = json
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in data.iter().take(ctx.limit) {
            let id = item
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|i| i.to_string())
                .unwrap_or_default();
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if id.is_empty() || title.is_empty() {
                continue;
            }
            let url = format!("https://sspai.com/post/{id}");
            let summary = item
                .get("summary")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let author = item
                .get("author")
                .and_then(|v| v.get("nickname"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let source_label = if !author.is_empty() {
                format!("{author} — 少数派 sspai")
            } else {
                "少数派 sspai".to_string()
            };
            hits.push(
                SearchHit::new(self.id(), title, url)
                    .with_description(summary)
                    .with_source(source_label),
            );
        }
        Ok(hits)
    }
}
