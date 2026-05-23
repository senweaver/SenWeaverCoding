// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::{clean_text, truncate_chars};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static ZH_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div[^>]*class="[^"]*List-item[^"]*"[^>]*>([\s\S]*?)</div>\s*</div>\s*</div>"#)
        .expect("zhihu item regex")
});
static ZH_TITLE_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<h2[^>]*class="[^"]*ContentItem-title[^"]*"[^>]*>[\s\S]*?<a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#)
        .expect("zhihu title link regex")
});
static ZH_CONTENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div[^>]*class="[^"]*RichContent-inner[^"]*"[^>]*>([\s\S]*?)</div>"#)
        .expect("zhihu rich content regex")
});

pub struct ZhihuEngine;

#[async_trait]
impl SearchEngine for ZhihuEngine {
    fn id(&self) -> &'static str {
        "zhihu"
    }

    fn label(&self) -> &'static str {
        "Zhihu"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Cn, SearchCategory::Social, SearchCategory::Web]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://www.zhihu.com/search?type=content&q={encoded}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Referer", "https://www.zhihu.com/")
            .send()
            .await?;
        let status = response.status();
        let html = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Zhihu search failed with status: {status}");
        }
        if html.contains("safety-center") || html.contains("/captcha?") {
            anyhow::bail!("Zhihu blocked the request with a security challenge");
        }
        let mut hits = Vec::new();
        for item_caps in ZH_ITEM_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let block = &item_caps[1];
            let Some(link_caps) = ZH_TITLE_LINK_RE.captures(block) else {
                continue;
            };
            let raw_url = link_caps[1].trim().to_string();
            let title = clean_text(&link_caps[2]);
            if title.is_empty() {
                continue;
            }
            let full_url = if raw_url.starts_with("http") {
                raw_url
            } else if raw_url.starts_with("//") {
                format!("https:{raw_url}")
            } else if raw_url.starts_with('/') {
                format!("https://www.zhihu.com{raw_url}")
            } else {
                raw_url
            };
            let content = ZH_CONTENT_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            hits.push(
                SearchHit::new(self.id(), title, full_url)
                    .with_description(truncate_chars(&content, 320))
                    .with_source("Zhihu"),
            );
        }
        Ok(hits)
    }
}
