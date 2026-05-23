// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct StackOverflowEngine;

#[async_trait]
impl SearchEngine for StackOverflowEngine {
    fn id(&self) -> &'static str {
        "stackoverflow"
    }

    fn label(&self) -> &'static str {
        "Stack Overflow"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Forum, SearchCategory::Code]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let pagesize = ctx.limit.clamp(5, 30);
        let url = format!(
            "https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=relevance&q={encoded}&site=stackoverflow&pagesize={pagesize}&filter=withbody"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Stack Overflow search failed with status: {}",
                response.status()
            );
        }
        let json: serde_json::Value = response.json().await?;
        let items = json
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let url_s = item
                .get("link")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if url_s.is_empty() {
                continue;
            }
            let body = item
                .get("body")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let snippet = if body.chars().count() > 240 {
                let truncated: String = body.chars().take(240).collect();
                format!("{truncated}…")
            } else {
                body
            };
            let owner = item
                .get("owner")
                .and_then(|v| v.get("display_name"))
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let score = item.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
            let answer_count = item.get("answer_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let is_answered = item.get("is_answered").and_then(|v| v.as_bool()).unwrap_or(false);
            let badges = if is_answered {
                format!("👍 {score} · ✔ {answer_count} answer(s)")
            } else {
                format!("👍 {score} · {answer_count} answer(s)")
            };
            let source_label = if !owner.is_empty() {
                format!("{owner} — Stack Overflow · {badges}")
            } else {
                format!("Stack Overflow · {badges}")
            };
            let published = item
                .get("creation_date")
                .and_then(|v| v.as_i64())
                .map(format_unix_timestamp);
            let mut hit = SearchHit::new(self.id(), title, url_s)
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

fn format_unix_timestamp(ts: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
