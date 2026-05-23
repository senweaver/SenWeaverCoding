// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct RedditEngine;

#[async_trait]
impl SearchEngine for RedditEngine {
    fn id(&self) -> &'static str {
        "reddit"
    }

    fn label(&self) -> &'static str {
        "Reddit"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Social, SearchCategory::Forum]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(5, 50);
        let subreddit = ctx
            .extra_str("subreddit")
            .map(|s| s.trim().trim_start_matches("r/").trim_start_matches('/').to_string())
            .filter(|s| !s.is_empty());
        let sort = ctx
            .extra_str("sort")
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| {
                matches!(
                    s.as_str(),
                    "relevance" | "hot" | "top" | "new" | "comments"
                )
            })
            .unwrap_or_else(|| "relevance".to_string());
        let time_filter = ctx
            .extra_str("time_filter")
            .or_else(|| ctx.extra_str("t"))
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| {
                matches!(
                    s.as_str(),
                    "hour" | "day" | "week" | "month" | "year" | "all"
                )
            });
        let mut url = if let Some(sub) = subreddit.as_deref() {
            format!(
                "https://www.reddit.com/r/{}/search.json?q={encoded}&limit={limit}&type=link&sort={sort}&restrict_sr=on",
                urlencoding::encode(sub)
            )
        } else {
            format!(
                "https://www.reddit.com/search.json?q={encoded}&limit={limit}&type=link&sort={sort}"
            )
        };
        if let Some(t) = time_filter {
            url.push_str(&format!("&t={}", urlencoding::encode(&t)));
        }
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Reddit search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let children = json
            .get("data")
            .and_then(|v| v.get("children"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for child in children.iter().take(ctx.limit) {
            let data = child.get("data").unwrap_or(child);
            let title = data
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let permalink = data
                .get("permalink")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if permalink.is_empty() {
                continue;
            }
            let target_url = format!("https://www.reddit.com{permalink}");
            let selftext = data
                .get("selftext")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let snippet = if selftext.chars().count() > 240 {
                let truncated: String = selftext.chars().take(240).collect();
                format!("{truncated}…")
            } else {
                selftext
            };
            let subreddit = data
                .get("subreddit_name_prefixed")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let author = data
                .get("author")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let score = data.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
            let num_comments = data.get("num_comments").and_then(|v| v.as_i64()).unwrap_or(0);
            let source_label = match (subreddit.is_empty(), author.is_empty()) {
                (false, false) => {
                    format!("u/{author} in {subreddit} — Reddit · ▲ {score} · 💬 {num_comments}")
                }
                (false, true) => format!("{subreddit} — Reddit · ▲ {score} · 💬 {num_comments}"),
                (true, false) => format!("u/{author} — Reddit · ▲ {score} · 💬 {num_comments}"),
                _ => format!("Reddit · ▲ {score} · 💬 {num_comments}"),
            };
            let published = data
                .get("created_utc")
                .and_then(|v| v.as_f64())
                .map(|f| f as i64)
                .map(|ts| {
                    use chrono::{TimeZone, Utc};
                    Utc.timestamp_opt(ts, 0)
                        .single()
                        .map(|dt| dt.format("%Y-%m-%d").to_string())
                        .unwrap_or_default()
                });
            let mut hit = SearchHit::new(self.id(), title, target_url)
                .with_description(snippet)
                .with_source(source_label);
            if let Some(p) = published {
                if !p.is_empty() {
                    hit = hit.with_published_at(p);
                }
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
