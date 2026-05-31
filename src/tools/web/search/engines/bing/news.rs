// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static BNEWS_CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]*class="[^"]*news-card[^"]*"[^>]*data-url="([^"]+)(.+?)</div>\s*</div>"#,
        "bing news card regex",
    )
});
static BNEWS_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<a[^>]*class="title(.+?)</a>"#,
        "bing news title regex",
    )
});
static BNEWS_SNIPPET_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]*class="snippet(.+?)</div>"#,
        "bing news snippet regex",
    )
});
static BNEWS_SOURCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]*class="source[^"]*(.+?)</div>"#,
        "bing news source regex",
    )
});

pub struct BingNewsEngine;

#[async_trait]
impl SearchEngine for BingNewsEngine {
    fn id(&self) -> &'static str {
        "bing_news"
    }

    fn label(&self) -> &'static str {
        "Bing News"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::News]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!(
            "https://www.bing.com/news/search?q={encoded}&qft=&FORM=HDRSC6"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header(
                "Cookie",
                "SRCHHPGROWTH=0; _EDGE_S=F=1; MUID=00000000000000000000000000000000",
            )
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Bing News search failed with status: {}",
                response.status()
            );
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        for caps in BNEWS_CARD_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let target_url = caps[1].trim().to_string();
            let block = &caps[2];
            let title = BNEWS_TITLE_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            if title.is_empty() || !target_url.starts_with("http") {
                continue;
            }
            let snippet = BNEWS_SNIPPET_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let source = BNEWS_SOURCE_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_else(|| "Bing News".to_string());
            hits.push(
                SearchHit::new(self.id(), title, target_url)
                    .with_description(snippet)
                    .with_source(source),
            );
        }
        Ok(hits)
    }
}
