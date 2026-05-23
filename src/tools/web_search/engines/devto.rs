// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct DevToEngine;

#[async_trait]
impl SearchEngine for DevToEngine {
    fn id(&self) -> &'static str {
        "dev_to"
    }

    fn label(&self) -> &'static str {
        "Dev.to"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Forum, SearchCategory::Web]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let per_page = ctx.limit.clamp(5, 30);
        let url = format!(
            "https://dev.to/api/articles?per_page={per_page}&top=7&tag_names={encoded}"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        let mut articles_array: Vec<serde_json::Value> = Vec::new();
        if response.status().is_success() {
            if let Ok(arr) = response.json::<Vec<serde_json::Value>>().await {
                articles_array = arr;
            }
        }
        // Fallback to keyword search using Algolia-like endpoint via search articles
        if articles_array.is_empty() {
            let search_url = format!(
                "https://dev.to/search/feed_content?per_page={per_page}&search_fields=tag_list,title,body_text&class_name=Article&q={encoded}"
            );
            let r2 = ctx
                .build_http_client()?
                .get(&search_url)
                .header("Accept", "application/json")
                .send()
                .await?;
            if r2.status().is_success() {
                if let Ok(json) = r2.json::<serde_json::Value>().await {
                    if let Some(results) = json
                        .get("result")
                        .and_then(|v| v.as_array())
                        .cloned()
                    {
                        articles_array = results;
                    }
                }
            }
        }
        if articles_array.is_empty() {
            anyhow::bail!("Dev.to returned no usable results");
        }
        let mut hits = Vec::new();
        for item in articles_array.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let target_url = item
                .get("url")
                .or_else(|| item.get("canonical_url"))
                .or_else(|| item.get("path"))
                .and_then(|v| v.as_str())
                .map(|s| {
                    if s.starts_with("http") {
                        s.to_string()
                    } else {
                        format!("https://dev.to{s}")
                    }
                })
                .unwrap_or_default();
            if target_url.is_empty() {
                continue;
            }
            let description = item
                .get("description")
                .or_else(|| item.get("body_text"))
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let snippet = if description.chars().count() > 240 {
                let truncated: String = description.chars().take(240).collect();
                format!("{truncated}…")
            } else {
                description
            };
            let author = item
                .get("user")
                .and_then(|v| v.get("name").or_else(|| v.get("username")))
                .or_else(|| item.get("user_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let reactions = item
                .get("public_reactions_count")
                .or_else(|| item.get("positive_reactions_count"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let source_label = if !author.is_empty() {
                format!("{author} — Dev.to · ❤ {reactions}")
            } else {
                format!("Dev.to · ❤ {reactions}")
            };
            let published = item
                .get("readable_publish_date")
                .or_else(|| item.get("published_at"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut hit = SearchHit::new(self.id(), title, target_url)
                .with_description(snippet)
                .with_source(source_label);
            if let Some(p) = published {
                hit = hit.with_published_at(p);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
