// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::{clean_text, truncate_chars};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static ARXIV_ENTRY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<entry>(.*?)</entry>").expect("arxiv entry regex")
});
static ARXIV_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<title>(.*?)</title>").expect("arxiv title regex")
});
static ARXIV_SUMMARY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<summary>(.*?)</summary>").expect("arxiv summary regex")
});
static ARXIV_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<id>(.*?)</id>").expect("arxiv id regex")
});
static ARXIV_PUBLISHED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<published>(.*?)</published>").expect("arxiv published regex")
});
static ARXIV_AUTHOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<author>\s*<name>(.*?)</name>").expect("arxiv author regex")
});

pub struct ArxivEngine;

#[async_trait]
impl SearchEngine for ArxivEngine {
    fn id(&self) -> &'static str {
        "arxiv"
    }

    fn label(&self) -> &'static str {
        "arXiv"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(1, 30);
        let url = format!(
            "https://export.arxiv.org/api/query?search_query=all:{encoded}&start=0&max_results={limit}&sortBy=relevance&sortOrder=descending"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/atom+xml")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("arXiv search failed with status: {}", response.status());
        }
        let xml = response.text().await?;
        let mut hits = Vec::new();
        for entry_caps in ARXIV_ENTRY_RE.captures_iter(&xml) {
            if hits.len() >= ctx.limit {
                break;
            }
            let entry = &entry_caps[1];
            let title = ARXIV_TITLE_RE
                .captures(entry)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let url = ARXIV_ID_RE
                .captures(entry)
                .map(|c| c[1].trim().to_string())
                .unwrap_or_default();
            let summary = ARXIV_SUMMARY_RE
                .captures(entry)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let published = ARXIV_PUBLISHED_RE
                .captures(entry)
                .map(|c| c[1].trim().to_string())
                .unwrap_or_default();
            let mut authors: Vec<String> = ARXIV_AUTHOR_RE
                .captures_iter(entry)
                .map(|c| c[1].trim().to_string())
                .collect();
            authors.truncate(4);
            let mut desc = String::new();
            if !authors.is_empty() {
                desc.push_str(&format!("[{}] ", authors.join(", ")));
            }
            if !summary.is_empty() {
                desc.push_str(&summary);
            }
            let mut hit = SearchHit::new(self.id(), title, url)
                .with_description(truncate_chars(&desc, 320))
                .with_source("arXiv");
            if !published.is_empty() {
                hit = hit.with_published_at(published);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
