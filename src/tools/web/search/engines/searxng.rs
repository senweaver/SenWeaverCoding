// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit};
use async_trait::async_trait;

pub struct SearXNGEngine;

#[async_trait]
impl SearchEngine for SearXNGEngine {
    fn id(&self) -> &'static str {
        "searxng"
    }

    fn label(&self) -> &'static str {
        "SearXNG"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::News, SearchCategory::Academic]
    }

    fn requires_api_key(&self) -> bool {
        true
    }

    fn is_available(&self, keys: &ApiKeys) -> bool {
        keys.searxng_url.as_ref().is_some_and(|k| !k.is_empty())
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let instance = ctx
            .api_keys
            .searxng_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("SearXNG instance URL not configured"))?;
        let base = instance.trim_end_matches('/');
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("{base}/search?q={encoded}&format=json&pageno=1");

        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("SearXNG search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let results = json
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid SearXNG API response"))?;
        let mut hits = Vec::new();
        for result in results.iter().take(ctx.limit) {
            let title = result
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let url = result
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            let content = result
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            if title.is_empty() && url.is_empty() {
                continue;
            }
            hits.push(SearchHit::new(self.id(), title, url).with_description(content));
        }
        Ok(hits)
    }
}
