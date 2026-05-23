// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static SF_CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<div[^>]*class="[^"]*search-item[^"]*"[^>]*>[\s\S]*?<a[^>]*href="(/a/[^"]+|/q/[^"]+)"[^>]*>([\s\S]*?)</a>[\s\S]*?<div[^>]*class="[^"]*search-item__desc[^"]*"[^>]*>([\s\S]*?)</div>"#,
    )
    .expect("segmentfault card regex")
});

pub struct SegmentFaultEngine;

#[async_trait]
impl SearchEngine for SegmentFaultEngine {
    fn id(&self) -> &'static str {
        "segmentfault"
    }

    fn label(&self) -> &'static str {
        "SegmentFault"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Forum, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://segmentfault.com/search?q={encoded}&type=article");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "SegmentFault search failed with status: {}",
                response.status()
            );
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        for caps in SF_CARD_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let path = caps[1].trim();
            let title = clean_text(&caps[2]);
            let desc = clean_text(&caps[3]);
            if title.is_empty() {
                continue;
            }
            let target_url = format!("https://segmentfault.com{path}");
            hits.push(
                SearchHit::new(self.id(), title, target_url)
                    .with_description(desc)
                    .with_source("SegmentFault 思否".to_string()),
            );
        }
        Ok(hits)
    }
}
