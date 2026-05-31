// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static YAHOO_NEWS_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<li[^>]*class="[^"]*ov[^"]*(.+?)</li>"#,
        "yahoo news item regex",
    )
});
static YAHOO_NEWS_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<h4[^>]*>\s*<a[^>]*href="([^"]+)(.+?)</a>"#,
        "yahoo news link regex",
    )
});
static YAHOO_NEWS_DESC_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<p[^>]*class="[^"]*s-desc[^"]*(.+?)</p>"#,
        "yahoo news desc regex",
    )
});
static YAHOO_NEWS_META_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<span[^>]*class="[^"]*s-source[^"]*(.+?)</span>"#,
        "yahoo news meta regex",
    )
});

pub struct YahooNewsEngine;

#[async_trait]
impl SearchEngine for YahooNewsEngine {
    fn id(&self) -> &'static str {
        "yahoo_news"
    }

    fn label(&self) -> &'static str {
        "Yahoo News"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::News]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://news.search.yahoo.com/search?p={encoded}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Yahoo News search failed with status: {}",
                response.status()
            );
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        for caps in YAHOO_NEWS_ITEM_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let block = &caps[1];
            let Some(link_caps) = YAHOO_NEWS_LINK_RE.captures(block) else {
                continue;
            };
            let href = link_caps[1].trim().to_string();
            let title = clean_text(&link_caps[2]);
            if title.is_empty() || !href.starts_with("http") {
                continue;
            }
            let snippet = YAHOO_NEWS_DESC_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let source = YAHOO_NEWS_META_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_else(|| "Yahoo News".to_string());
            hits.push(
                SearchHit::new(self.id(), title, href)
                    .with_description(snippet)
                    .with_source(source),
            );
        }
        Ok(hits)
    }
}
