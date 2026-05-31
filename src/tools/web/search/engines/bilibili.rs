// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct BilibiliEngine;

#[async_trait]
impl SearchEngine for BilibiliEngine {
    fn id(&self) -> &'static str {
        "bilibili"
    }

    fn label(&self) -> &'static str {
        "Bilibili"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Video, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!(
            "https://api.bilibili.com/x/web-interface/wbi/search/type?search_type=video&keyword={encoded}&page=1&page_size={}",
            ctx.limit.clamp(5, 30)
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .header("Referer", "https://search.bilibili.com")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Bilibili search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let results = json
            .get("data")
            .and_then(|v| v.get("result"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in results.iter().take(ctx.limit) {
            let bvid = item.get("bvid").and_then(|v| v.as_str()).unwrap_or("");
            let aid = item
                .get("aid")
                .and_then(|v| v.as_i64())
                .map(|i| i.to_string())
                .unwrap_or_default();
            let title_html = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let title = clean_text(title_html);
            if title.is_empty() {
                continue;
            }
            let target_url = if !bvid.is_empty() {
                format!("https://www.bilibili.com/video/{bvid}")
            } else if !aid.is_empty() {
                format!("https://www.bilibili.com/video/av{aid}")
            } else {
                continue;
            };
            let desc_html = item.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let desc = clean_text(desc_html);
            let author = item
                .get("author")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let play = item.get("play").and_then(|v| v.as_i64()).unwrap_or(0);
            let danmaku = item.get("video_review").and_then(|v| v.as_i64()).unwrap_or(0);
            let duration = item
                .get("duration")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let play_short = format_count(play);
            let source_label = match (author.is_empty(), duration.is_empty()) {
                (false, false) => format!("UP: {author}  - Bilibili · ▲{play_short} · ⏰{duration} · 💬 {danmaku}"),
                (false, true) => format!("UP: {author}  - Bilibili · ▲{play_short}"),
                (true, false) => format!("Bilibili · ▲{play_short} · ⏰{duration}"),
                _ => format!("Bilibili · ▲{play_short}"),
            };
            let published = item
                .get("pubdate")
                .and_then(|v| v.as_i64())
                .map(|ts| {
                    use chrono::{TimeZone, Utc};
                    Utc.timestamp_opt(ts, 0)
                        .single()
                        .map(|dt| dt.format("%Y-%m-%d").to_string())
                        .unwrap_or_default()
                })
                .filter(|s| !s.is_empty());
            let mut hit = SearchHit::new(self.id(), title, target_url)
                .with_description(desc)
                .with_source(source_label);
            if let Some(p) = published {
                hit = hit.with_published_at(p);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}

fn format_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}w", n as f64 / 10_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
