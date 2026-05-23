// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static ITHOME_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<div[^>]*class="search-item"[^>]*>[\s\S]*?<a[^>]*href="(https?://[^"]+ithome[^"]+)"[^>]*>([\s\S]*?)</a>[\s\S]*?<div[^>]*class="memo"[^>]*>([\s\S]*?)</div>"#,
    )
    .expect("ithome item regex")
});

pub struct IthomeEngine;

#[async_trait]
impl SearchEngine for IthomeEngine {
    fn id(&self) -> &'static str {
        "ithome"
    }

    fn label(&self) -> &'static str {
        "IT之家"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Lifestyle, SearchCategory::News, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://so.ithome.com/?Keyword={encoded}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("IT之家 search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        for caps in ITHOME_ITEM_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let url_str = caps[1].trim().to_string();
            let title = clean_text(&caps[2]);
            let memo = clean_text(&caps[3]);
            if title.is_empty() {
                continue;
            }
            hits.push(
                SearchHit::new(self.id(), title, url_str)
                    .with_description(memo)
                    .with_source("IT之家".to_string()),
            );
        }
        Ok(hits)
    }
}
