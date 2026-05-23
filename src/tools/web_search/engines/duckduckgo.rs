// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::{clean_text, decode_ddg_redirect_url, strip_tags};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static DDG_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#)
        .expect("ddg link regex")
});
static DDG_SNIPPET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a class="result__snippet[^"]*"[^>]*>([\s\S]*?)</a>"#)
        .expect("ddg snippet regex")
});

pub struct DuckDuckGoEngine;

#[async_trait]
impl SearchEngine for DuckDuckGoEngine {
    fn id(&self) -> &'static str {
        "duckduckgo"
    }

    fn label(&self) -> &'static str {
        "DuckDuckGo"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::News]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let body = format!("q={encoded}&kl=wt-wt");
        let client = ctx.build_http_client()?;
        let response = client
            .post("https://html.duckduckgo.com/html/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.9,zh-CN;q=0.7")
            .header("Origin", "https://html.duckduckgo.com")
            .header("Referer", "https://html.duckduckgo.com/")
            .body(body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("DuckDuckGo search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        let snippet_matches: Vec<_> = DDG_SNIPPET_RE
            .captures_iter(&html)
            .take(ctx.limit + 4)
            .collect();
        for (i, caps) in DDG_LINK_RE
            .captures_iter(&html)
            .take(ctx.limit)
            .enumerate()
        {
            let url_str = decode_ddg_redirect_url(&caps[1]);
            let title = clean_text(&strip_tags(&caps[2]));
            if title.is_empty() || url_str.trim().is_empty() {
                continue;
            }
            let snippet = snippet_matches
                .get(i)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            hits.push(
                SearchHit::new(self.id(), title, url_str.trim()).with_description(snippet),
            );
        }
        Ok(hits)
    }
}
