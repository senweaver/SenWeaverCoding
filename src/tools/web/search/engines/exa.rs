// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit};
use async_trait::async_trait;
use serde_json::json;

pub struct ExaEngine;

#[async_trait]
impl SearchEngine for ExaEngine {
    fn id(&self) -> &'static str {
        "exa"
    }

    fn label(&self) -> &'static str {
        "Exa"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::Academic]
    }

    fn requires_api_key(&self) -> bool {
        true
    }

    fn is_available(&self, keys: &ApiKeys) -> bool {
        keys.exa.as_ref().is_some_and(|k| !k.is_empty())
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let api_key = ctx
            .api_keys
            .exa
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Exa API key not configured"))?;
        let default_kind = if matches!(ctx.category, SearchCategory::Academic) {
            "neural"
        } else {
            "auto"
        };
        let kind = ctx
            .extra_str("exa_type")
            .or_else(|| ctx.extra_str("type"))
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| matches!(s.as_str(), "neural" | "keyword" | "auto"))
            .unwrap_or_else(|| default_kind.to_string());
        let get_contents = ctx.extra_bool("get_contents").unwrap_or(false);
        let highlight_sentences = ctx
            .extra_i64("highlight_sentences")
            .unwrap_or(3)
            .clamp(1, 12);
        let mut contents = json!({
            "text": get_contents,
            "highlights": { "numSentences": highlight_sentences },
        });
        if let Some(category) = ctx.extra_str("category_filter") {
            if !category.trim().is_empty() {
                if let Some(obj) = contents.as_object_mut() {
                    obj.insert("category".into(), json!(category));
                }
            }
        }
        let mut body = json!({
            "query": ctx.query,
            "type": kind,
            "numResults": ctx.limit.clamp(1, 30) as i64,
            "contents": contents,
        });
        let body_obj = body
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("exa: failed to construct request JSON body as object"))?;
        if let Some(category) = ctx.extra_str("category_filter") {
            if !category.trim().is_empty() {
                body_obj.insert("category".into(), json!(category));
            }
        }
        if let Some(arr) = ctx.extra.get("include_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            if !domains.is_empty() {
                body_obj.insert("includeDomains".into(), json!(domains));
            }
        }
        if let Some(arr) = ctx.extra.get("exclude_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            if !domains.is_empty() {
                body_obj.insert("excludeDomains".into(), json!(domains));
            }
        }
        let client = ctx.build_http_client()?;
        let response = client
            .post("https://api.exa.ai/search")
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Exa search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let mut hits = Vec::new();
        if let Some(results) = json.get("results").and_then(|v| v.as_array()) {
            for item in results.iter().take(ctx.limit) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if title.is_empty() || url.is_empty() {
                    continue;
                }
                let snippet = item
                    .get("highlights")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|h| h.as_str())
                            .collect::<Vec<_>>()
                            .join("  - ")
                    })
                    .or_else(|| item.get("text").and_then(|v| v.as_str()).map(str::to_string))
                    .unwrap_or_default();
                let author = item
                    .get("author")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let published = item
                    .get("publishedDate")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut hit = SearchHit::new(self.id(), title, url).with_description(snippet);
                if !author.is_empty() {
                    hit = hit.with_source(author);
                }
                if !published.is_empty() {
                    hit = hit.with_published_at(published);
                }
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}
