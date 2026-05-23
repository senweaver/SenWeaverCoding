// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static GNEWS_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<item>([\s\S]*?)</item>"#).expect("gnews item regex")
});
static GNEWS_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<title>([\s\S]*?)</title>"#).expect("gnews title regex"));
static GNEWS_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<link>([\s\S]*?)</link>"#).expect("gnews link regex"));
static GNEWS_PUBDATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<pubDate>([\s\S]*?)</pubDate>"#).expect("gnews date regex"));
static GNEWS_DESC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<description>([\s\S]*?)</description>"#).expect("gnews desc regex")
});
static GNEWS_SOURCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<source[^>]*>([\s\S]*?)</source>"#).expect("gnews source regex")
});

fn strip_cdata(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(stripped) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
    {
        return stripped.to_string();
    }
    trimmed.to_string()
}

pub struct GoogleNewsEngine;

#[async_trait]
impl SearchEngine for GoogleNewsEngine {
    fn id(&self) -> &'static str {
        "google_news"
    }

    fn label(&self) -> &'static str {
        "Google News"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::News]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let locale = ctx
            .locale
            .clone()
            .unwrap_or_else(|| "zh-CN".to_string());
        let (hl, gl, ceid) = match locale.to_lowercase().as_str() {
            "en" | "en-us" | "us" => ("en-US", "US", "US:en"),
            "ja" | "ja-jp" | "jp" => ("ja", "JP", "JP:ja"),
            _ => ("zh-CN", "CN", "CN:zh-Hans"),
        };
        let url = format!(
            "https://news.google.com/rss/search?q={encoded}&hl={hl}&gl={gl}&ceid={ceid}"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/rss+xml, application/xml, text/xml")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Google News search failed with status: {}",
                response.status()
            );
        }
        let xml = response.text().await?;
        let mut hits = Vec::new();
        for item_caps in GNEWS_ITEM_RE.captures_iter(&xml) {
            if hits.len() >= ctx.limit {
                break;
            }
            let item = &item_caps[1];
            let title = GNEWS_TITLE_RE
                .captures(item)
                .map(|c| clean_text(&strip_cdata(&c[1])))
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let link = GNEWS_LINK_RE
                .captures(item)
                .map(|c| strip_cdata(&c[1]).trim().to_string())
                .unwrap_or_default();
            if link.is_empty() || !link.starts_with("http") {
                continue;
            }
            let description = GNEWS_DESC_RE
                .captures(item)
                .map(|c| clean_text(&strip_cdata(&c[1])))
                .unwrap_or_default();
            let published = GNEWS_PUBDATE_RE
                .captures(item)
                .map(|c| strip_cdata(&c[1]).trim().to_string())
                .filter(|s| !s.is_empty());
            let source = GNEWS_SOURCE_RE
                .captures(item)
                .map(|c| clean_text(&strip_cdata(&c[1])))
                .unwrap_or_else(|| "Google News".to_string());
            let mut hit = SearchHit::new(self.id(), title, link)
                .with_description(description)
                .with_source(source);
            if let Some(p) = published {
                hit = hit.with_published_at(p);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
