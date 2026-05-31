// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static SOHU_CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<a[^>]*href="(https?://[^"]+sohu\.com/[^"]+)"[^>]*target="_blank(.+?)</a>"#,
        "sohu card regex",
    )
});

pub struct SohuEngine;

#[async_trait]
impl SearchEngine for SohuEngine {
    fn id(&self) -> &'static str {
        "sohu"
    }

    fn label(&self) -> &'static str {
        "搜狐"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::News, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!(
            "https://search.sohu.com/?queryType=outside&keyword={encoded}&spm=smpc.csrpage.search-result.1"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "text/html")
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Sohu search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        let mut hits = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for caps in SOHU_CARD_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let url_str = caps[1].trim().to_string();
            if !seen.insert(url_str.clone()) {
                continue;
            }
            let title = clean_text(&caps[2]);
            if title.is_empty() {
                continue;
            }
            hits.push(
                SearchHit::new(self.id(), title, url_str).with_source("搜狐".to_string()),
            );
        }
        Ok(hits)
    }
}
