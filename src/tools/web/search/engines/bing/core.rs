// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static BING_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<li[^>]*class="[^"]*b_algo[^"]*(.+?)</li>"#,
        "bing block regex",
    )
});
static BING_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<h2[^>]*>[\s\S]*?<a[^>]*href="([^"]+)(.+?)</a>[\s\S]*?</h2>"#,
        "bing link regex",
    )
});
static BING_SNIPPET_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<(?:p|div)[^>]*class="[^"]*(?:b_lineclamp|b_caption|b_snippet|b_paractl)[^"]*(.+?)</(?:p|div)>"#,
        "bing snippet regex",
    )
});
static BING_SOURCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(r"</cite>(.+?)</cite>", "bing source regex")
});

pub struct BingEngine;

#[async_trait]
impl SearchEngine for BingEngine {
    fn id(&self) -> &'static str {
        "bing"
    }

    fn label(&self) -> &'static str {
        "Bing"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::News]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!(
            "https://www.bing.com/search?q={encoded}&count={}&first=1",
            ctx.limit
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Host", "www.bing.com")
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Accept-Encoding", "gzip, deflate, br")
            .header("Cache-Control", "no-cache")
            .header("Pragma", "no-cache")
            .header(
                "sec-ch-ua",
                "\"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\", \"Not:A-Brand\";v=\"99\"",
            )
            .header("sec-ch-ua-mobile", "?0")
            .header("sec-ch-ua-platform", "\"Windows\"")
            .header("sec-fetch-dest", "document")
            .header("sec-fetch-mode", "navigate")
            .header("sec-fetch-site", "none")
            .header("sec-fetch-user", "?1")
            .header("upgrade-insecure-requests", "1")
            .header(
                "Cookie",
                "SRCHHPGROWTH=0; _EDGE_S=F=1; MUID=00000000000000000000000000000000",
            )
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Bing search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        for caps in BING_BLOCK_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let block = &caps[1];
            let Some(link_caps) = BING_LINK_RE.captures(block) else {
                continue;
            };
            let url_str = link_caps[1].trim().to_string();
            let title = clean_text(&link_caps[2]);
            if title.is_empty() || !url_str.starts_with("http") {
                continue;
            }
            let snippet = BING_SNIPPET_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let source = BING_SOURCE_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let mut hit = SearchHit::new(self.id(), title, url_str).with_description(snippet);
            if !source.is_empty() {
                hit = hit.with_source(source);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
