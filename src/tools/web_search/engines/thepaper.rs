// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static THEPAPER_CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a[^>]*href="(/newsDetail_forward_[^"]+)"[^>]*>([\s\S]*?)</a>"#)
        .expect("thepaper card regex")
});

pub struct ThePaperEngine;

#[async_trait]
impl SearchEngine for ThePaperEngine {
    fn id(&self) -> &'static str {
        "thepaper"
    }

    fn label(&self) -> &'static str {
        "澎湃新闻"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::News, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://www.thepaper.cn/searchResult.jsp?inpsearchString={encoded}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "ThePaper search failed with status: {}",
                response.status()
            );
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for caps in THEPAPER_CARD_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let path = caps[1].trim();
            if !seen.insert(path.to_string()) {
                continue;
            }
            let title = clean_text(&caps[2]);
            if title.is_empty() {
                continue;
            }
            let url = format!("https://www.thepaper.cn{path}");
            hits.push(
                SearchHit::new(self.id(), title, url).with_source("澎湃新闻".to_string()),
            );
        }
        Ok(hits)
    }
}
