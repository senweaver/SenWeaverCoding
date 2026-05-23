// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static BIORXIV_CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<li[^>]*class="[^"]*search-result[^"]*"[^>]*>([\s\S]*?)</li>"#)
        .expect("biorxiv card regex")
});
static BIORXIV_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<span[^>]*class="[^"]*highwire-cite-title[^"]*"[^>]*>[\s\S]*?<a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
    )
    .expect("biorxiv title regex")
});
static BIORXIV_AUTHORS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<span[^>]*class="[^"]*highwire-citation-authors[^"]*"[^>]*>([\s\S]*?)</span>"#)
        .expect("biorxiv authors regex")
});
static BIORXIV_DATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<span[^>]*class="[^"]*highwire-cite-metadata-date[^"]*"[^>]*>([\s\S]*?)</span>"#)
        .expect("biorxiv date regex")
});

pub struct BioRxivEngine;

#[async_trait]
impl SearchEngine for BioRxivEngine {
    fn id(&self) -> &'static str {
        "biorxiv"
    }

    fn label(&self) -> &'static str {
        "bioRxiv"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://www.biorxiv.org/search/{encoded}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("bioRxiv search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        for caps in BIORXIV_CARD_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let card = &caps[1];
            let Some(title_caps) = BIORXIV_TITLE_RE.captures(card) else {
                continue;
            };
            let href = title_caps[1].trim();
            let title = clean_text(&title_caps[2]);
            if title.is_empty() {
                continue;
            }
            let full_url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("https://www.biorxiv.org{href}")
            };
            let authors = BIORXIV_AUTHORS_RE
                .captures(card)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let published = BIORXIV_DATE_RE
                .captures(card)
                .map(|c| clean_text(&c[1]))
                .filter(|s| !s.is_empty());
            let source_label = if !authors.is_empty() {
                format!("{authors} — bioRxiv")
            } else {
                "bioRxiv".to_string()
            };
            let mut hit = SearchHit::new(self.id(), title, full_url).with_source(source_label);
            if let Some(p) = published {
                hit = hit.with_published_at(p);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
