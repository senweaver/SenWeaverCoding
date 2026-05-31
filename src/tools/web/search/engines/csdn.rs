// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct CsdnEngine;

#[async_trait]
impl SearchEngine for CsdnEngine {
    fn id(&self) -> &'static str {
        "csdn"
    }

    fn label(&self) -> &'static str {
        "CSDN"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Cn, SearchCategory::Web]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!(
            "https://so.csdn.net/api/v3/search?q={encoded}&t=blog&p=1&s=0&tm=0&lv=-1&ft=0"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", "https://so.csdn.net/")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("CSDN search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let mut hits = Vec::new();
        let items = json
            .get("result_vos")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        for item in items.iter().take(ctx.limit) {
            let url = item
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() || url.is_empty() {
                continue;
            }
            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("summary").and_then(|v| v.as_str()))
                .map(clean_text)
                .unwrap_or_default();
            hits.push(SearchHit::new(self.id(), title, url).with_description(description));
        }
        Ok(hits)
    }
}
