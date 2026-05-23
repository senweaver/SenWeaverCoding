// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static SSRN_ROW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<div[^>]*class="[^"]*description[^"]*"[^>]*>[\s\S]*?<a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>([\s\S]*?)</div>"#,
    )
    .expect("ssrn row regex")
});
static SSRN_AUTHORS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div[^>]*class="[^"]*authors[^"]*"[^>]*>([\s\S]*?)</div>"#)
        .expect("ssrn authors regex")
});

pub struct SsrnEngine;

#[async_trait]
impl SearchEngine for SsrnEngine {
    fn id(&self) -> &'static str {
        "ssrn"
    }

    fn label(&self) -> &'static str {
        "SSRN"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://papers.ssrn.com/sol3/results.cfm?txtKey_Words={encoded}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("SSRN search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        for caps in SSRN_ROW_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let href = caps[1].trim().to_string();
            let title = clean_text(&caps[2]);
            if title.is_empty() {
                continue;
            }
            let full_url = if href.starts_with("http") {
                href
            } else {
                format!("https://papers.ssrn.com{href}")
            };
            let tail = &caps[3];
            let authors = SSRN_AUTHORS_RE
                .captures(tail)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let source_label = if !authors.is_empty() {
                format!("{authors} — SSRN")
            } else {
                "SSRN".to_string()
            };
            hits.push(
                SearchHit::new(self.id(), title, full_url)
                    .with_source(source_label),
            );
        }
        Ok(hits)
    }
}
