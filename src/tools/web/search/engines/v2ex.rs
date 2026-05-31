// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct V2exEngine;

#[async_trait]
impl SearchEngine for V2exEngine {
    fn id(&self) -> &'static str {
        "v2ex"
    }

    fn label(&self) -> &'static str {
        "V2EX"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Forum, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let size = ctx.limit.clamp(5, 30);
        let url = format!("https://www.sov2ex.com/api/search?q={encoded}&size={size}&sort=sumup");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("V2EX search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let items = json
            .get("hits")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for hit in items.iter().take(ctx.limit) {
            let source = hit
                .get("_source")
                .or_else(|| hit.get("source"))
                .unwrap_or(hit);
            let id = source
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|i| i.to_string())
                .unwrap_or_default();
            let title = source
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() || id.is_empty() {
                continue;
            }
            let target_url = format!("https://www.v2ex.com/t/{id}");
            let content = source
                .get("content")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let snippet = if content.chars().count() > 240 {
                let truncated: String = content.chars().take(240).collect();
                format!("{truncated}...")
            } else {
                content
            };
            let author = source
                .get("member")
                .and_then(|v| v.get("username"))
                .or_else(|| source.get("username"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let replies = source.get("replies").and_then(|v| v.as_i64()).unwrap_or(0);
            let source_label = if !author.is_empty() {
                format!("{author}  - V2EX · 💬 {replies}")
            } else {
                format!("V2EX · 💬 {replies}")
            };
            let published = source
                .get("created")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut hit_obj = SearchHit::new(self.id(), title, target_url)
                .with_description(snippet)
                .with_source(source_label);
            if let Some(p) = published {
                hit_obj = hit_obj.with_published_at(p);
            }
            hits.push(hit_obj);
        }
        Ok(hits)
    }
}
