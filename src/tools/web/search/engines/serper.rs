// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit};
use async_trait::async_trait;
use serde_json::json;

pub struct SerperEngine;

#[async_trait]
impl SearchEngine for SerperEngine {
    fn id(&self) -> &'static str {
        "serper"
    }

    fn label(&self) -> &'static str {
        "Serper (Google)"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::News, SearchCategory::Academic]
    }

    fn requires_api_key(&self) -> bool {
        true
    }

    fn is_available(&self, keys: &ApiKeys) -> bool {
        keys.serper.as_ref().is_some_and(|k| !k.is_empty())
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let api_key = ctx
            .api_keys
            .serper
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Serper API key not configured"))?;
        let body = json!({
            "q": ctx.query,
            "num": ctx.limit.clamp(1, 30) as i64,
            "hl": ctx.locale.as_deref().unwrap_or("en"),
        });
        let client = ctx.build_http_client()?;
        let response = client
            .post("https://google.serper.dev/search")
            .header("X-API-KEY", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Serper search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let mut hits = Vec::new();
        if let Some(organic) = json.get("organic").and_then(|v| v.as_array()) {
            for item in organic.iter().take(ctx.limit) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = item
                    .get("link")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if title.is_empty() || url.is_empty() {
                    continue;
                }
                let snippet = item
                    .get("snippet")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                hits.push(
                    SearchHit::new(self.id(), title, url)
                        .with_description(snippet)
                        .with_source("Google"),
                );
            }
        }
        Ok(hits)
    }
}
