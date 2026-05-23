// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct HackerNewsEngine;

#[async_trait]
impl SearchEngine for HackerNewsEngine {
    fn id(&self) -> &'static str {
        "hackernews"
    }

    fn label(&self) -> &'static str {
        "Hacker News"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Forum, SearchCategory::News]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let hits_per = ctx.limit.clamp(5, 50);
        let url = format!(
            "https://hn.algolia.com/api/v1/search?query={encoded}&hitsPerPage={hits_per}&tags=story"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Hacker News search failed with status: {}",
                response.status()
            );
        }
        let json: serde_json::Value = response.json().await?;
        let items = json
            .get("hits")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .or_else(|| item.get("story_title"))
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let object_id = item
                .get("objectID")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target_url = item
                .get("url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    format!("https://news.ycombinator.com/item?id={object_id}")
                });
            if target_url.is_empty() {
                continue;
            }
            let story_text = item
                .get("story_text")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let author = item
                .get("author")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let points = item.get("points").and_then(|v| v.as_i64()).unwrap_or(0);
            let num_comments = item.get("num_comments").and_then(|v| v.as_i64()).unwrap_or(0);
            let source_label = if !author.is_empty() {
                format!("{author} — Hacker News · ▲ {points} · 💬 {num_comments}")
            } else {
                format!("Hacker News · ▲ {points} · 💬 {num_comments}")
            };
            let published = item
                .get("created_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut hit = SearchHit::new(self.id(), title, target_url)
                .with_description(story_text)
                .with_source(source_label);
            if let Some(p) = published {
                hit = hit.with_published_at(p);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
