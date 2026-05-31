// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct InvidiousEngine;

const DEFAULT_INSTANCES: &[&str] = &[
    "https://yewtu.be",
    "https://invidious.snopyta.org",
    "https://vid.puffyan.us",
    "https://invidious.flokinet.to",
];

#[async_trait]
impl SearchEngine for InvidiousEngine {
    fn id(&self) -> &'static str {
        "invidious"
    }

    fn label(&self) -> &'static str {
        "YouTube (Invidious)"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Video]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        if let Some(key) = ctx
            .api_keys
            .youtube_api_key
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            match search_via_youtube_data_v3(ctx, key).await {
                Ok(hits) if !hits.is_empty() => return Ok(hits),
                Ok(_) => {}
                Err(_) => {}
            }
        }
        let encoded = urlencoding::encode(&ctx.query);
        let configured = ctx
            .api_keys
            .invidious_instance
            .as_deref()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty());
        let instances: Vec<String> = configured
            .into_iter()
            .chain(DEFAULT_INSTANCES.iter().map(|s| s.to_string()))
            .collect();
        let sort_by = ctx
            .extra_str("sort_by")
            .map(|s| s.to_ascii_lowercase())
            .map(|s| match s.as_str() {
                "date" => "upload_date".to_string(),
                "views" => "view_count".to_string(),
                "rating" => "rating".to_string(),
                _ => "relevance".to_string(),
            })
            .unwrap_or_else(|| "relevance".to_string());
        let client = ctx.build_http_client()?;
        let mut last_err: Option<anyhow::Error> = None;
        for inst in instances {
            let url = format!(
                "{inst}/api/v1/search?q={encoded}&type=video&sort_by={}",
                urlencoding::encode(&sort_by)
            );
            let resp = match client
                .get(&url)
                .header("Accept", "application/json")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(anyhow::anyhow!("{inst}: {e}"));
                    continue;
                }
            };
            if !resp.status().is_success() {
                last_err = Some(anyhow::anyhow!(
                    "{inst}: status {}",
                    resp.status()
                ));
                continue;
            }
            let json: serde_json::Value = match resp.json().await {
                Ok(j) => j,
                Err(e) => {
                    last_err = Some(anyhow::anyhow!("{inst}: parse {e}"));
                    continue;
                }
            };
            let arr = json.as_array().cloned().unwrap_or_default();
            if arr.is_empty() {
                continue;
            }
            let mut hits = Vec::new();
            for item in arr.iter().take(ctx.limit) {
                let type_s = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if type_s != "video" {
                    continue;
                }
                let video_id = item
                    .get("videoId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if video_id.is_empty() {
                    continue;
                }
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(clean_text)
                    .unwrap_or_default();
                if title.is_empty() {
                    continue;
                }
                let target_url = format!("https://www.youtube.com/watch?v={video_id}");
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(clean_text)
                    .unwrap_or_default();
                let author = item
                    .get("author")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let views = item.get("viewCount").and_then(|v| v.as_i64()).unwrap_or(0);
                let len = item.get("lengthSeconds").and_then(|v| v.as_i64()).unwrap_or(0);
                let duration = format_seconds(len);
                let view_short = format_count(views);
                let source_label = match (author.is_empty(), duration.is_empty()) {
                    (false, false) => format!("{author}  - YouTube · ▲{view_short} · ⏰{duration}"),
                    (false, true) => format!("{author}  - YouTube · ▲{view_short}"),
                    (true, false) => format!("YouTube · ▲{view_short} · ⏰{duration}"),
                    _ => format!("YouTube · ▲{view_short}"),
                };
                let published = item
                    .get("publishedText")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let mut hit = SearchHit::new(self.id(), title, target_url)
                    .with_description(desc)
                    .with_source(source_label);
                if let Some(p) = published {
                    hit = hit.with_published_at(p);
                }
                hits.push(hit);
            }
            if !hits.is_empty() {
                return Ok(hits);
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No Invidious instance returned results")))
    }
}

async fn search_via_youtube_data_v3(
    ctx: &SearchContext,
    api_key: &str,
) -> anyhow::Result<Vec<SearchHit>> {
    let client = ctx.build_http_client()?;
    let max_results = ctx.limit.clamp(5, 50);
    let order = ctx
        .extra_str("sort_by")
        .map(|s| s.to_ascii_lowercase())
        .map(|s| match s.as_str() {
            "date" => "date".to_string(),
            "views" => "viewCount".to_string(),
            "rating" => "rating".to_string(),
            _ => "relevance".to_string(),
        })
        .unwrap_or_else(|| "relevance".to_string());
    let url = format!(
        "https://www.googleapis.com/youtube/v3/search?part=snippet&type=video&maxResults={max_results}&order={}&q={}&key={}",
        urlencoding::encode(&order),
        urlencoding::encode(&ctx.query),
        urlencoding::encode(api_key)
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "YouTube Data API v3 failed with status {}",
            resp.status()
        );
    }
    let json: serde_json::Value = resp.json().await?;
    if let Some(err) = json.get("error") {
        let msg = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        anyhow::bail!("YouTube Data API v3 error: {msg}");
    }
    let items = json
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut hits = Vec::new();
    for item in items.iter().take(ctx.limit) {
        let video_id = item
            .get("id")
            .and_then(|id| id.get("videoId"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if video_id.is_empty() {
            continue;
        }
        let snippet = item.get("snippet").unwrap_or(item);
        let title = snippet
            .get("title")
            .and_then(|v| v.as_str())
            .map(crate::tools::web::search::parsers::clean_text)
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let channel = snippet
            .get("channelTitle")
            .and_then(|v| v.as_str())
            .map(crate::tools::web::search::parsers::clean_text)
            .unwrap_or_default();
        let description = snippet
            .get("description")
            .and_then(|v| v.as_str())
            .map(crate::tools::web::search::parsers::clean_text)
            .unwrap_or_default();
        let published = snippet
            .get("publishedAt")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let target_url = format!("https://www.youtube.com/watch?v={video_id}");
        let source_label = if channel.is_empty() {
            "YouTube".to_string()
        } else {
            format!("{channel}  - YouTube")
        };
        let mut hit = SearchHit::new("invidious", title, target_url)
            .with_description(description)
            .with_source(source_label);
        if let Some(p) = published {
            hit = hit.with_published_at(p);
        }
        hits.push(hit);
    }
    Ok(hits)
}

fn format_seconds(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn format_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
