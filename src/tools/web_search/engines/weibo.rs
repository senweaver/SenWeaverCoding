// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static WEIBO_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<div[^>]*class="card-wrap"[^>]*action-type="feed_list_item"[^>]*>[\s\S]*?<p[^>]*class="txt"[^>]*nick-name="([^"]*)"[^>]*>([\s\S]*?)</p>[\s\S]*?<a[^>]*href="(//weibo\.com/[^"]+)"[^>]*>[\s\S]*?</a>"#,
    )
    .expect("weibo item regex")
});

pub struct WeiboEngine;

#[async_trait]
impl SearchEngine for WeiboEngine {
    fn id(&self) -> &'static str {
        "weibo"
    }

    fn label(&self) -> &'static str {
        "微博"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Social, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://s.weibo.com/weibo?q={encoded}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Weibo search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for caps in WEIBO_ITEM_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let nick = clean_text(&caps[1]);
            let text = clean_text(&caps[2]);
            let href = caps[3].trim();
            if text.is_empty() {
                continue;
            }
            let full_url = format!("https:{href}");
            if !seen.insert(full_url.clone()) {
                continue;
            }
            let title = if text.chars().count() > 60 {
                let truncated: String = text.chars().take(60).collect();
                format!("{truncated}…")
            } else {
                text.clone()
            };
            let source_label = if !nick.is_empty() {
                format!("{nick} — 微博")
            } else {
                "微博".to_string()
            };
            hits.push(
                SearchHit::new(self.id(), title, full_url)
                    .with_description(text)
                    .with_source(source_label),
            );
        }
        Ok(hits)
    }
}
