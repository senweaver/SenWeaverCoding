// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct CoreEngine;

#[async_trait]
impl SearchEngine for CoreEngine {
    fn id(&self) -> &'static str {
        "core"
    }

    fn label(&self) -> &'static str {
        "CORE"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    fn requires_api_key(&self) -> bool {
        false
    }

    fn is_available(&self, _keys: &ApiKeys) -> bool {
        true
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let limit = ctx.limit.clamp(5, 30);
        let body = serde_json::json!({
            "q": ctx.query,
            "limit": limit,
            "scroll": false,
        });
        let client = ctx.build_http_client()?;
        let mut req = client
            .post("https://api.core.ac.uk/v3/search/works")
            .header("Accept", "application/json")
            .json(&body);
        if let Some(key) = ctx.api_keys.core.as_ref().filter(|s| !s.is_empty()) {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        let response = req.send().await?;
        if !response.status().is_success() {
            anyhow::bail!("CORE search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in results.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let mut url = item
                .get("downloadUrl")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if url.is_empty() {
                url = item
                    .get("sourceFulltextUrls")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
            }
            if url.is_empty() {
                if let Some(doi) = item.get("doi").and_then(|v| v.as_str()) {
                    if !doi.trim().is_empty() {
                        let normalized = doi
                            .trim()
                            .trim_start_matches("https://doi.org/")
                            .trim_start_matches("http://doi.org/");
                        url = format!("https://doi.org/{normalized}");
                    }
                }
            }
            if url.is_empty() {
                if let Some(id) = item.get("id").and_then(|v| v.as_i64()) {
                    url = format!("https://core.ac.uk/works/{id}");
                }
            }
            if url.is_empty() {
                continue;
            }
            let abstract_s = item
                .get("abstract")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let authors = item
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
                        .take(4)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let published = item
                .get("publishedDate")
                .or_else(|| item.get("yearPublished"))
                .and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                });
            let doi = item
                .get("doi")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let source_label = if !authors.is_empty() {
                format!("{authors} — CORE")
            } else {
                "CORE".to_string()
            };
            let mut hit = SearchHit::new(self.id(), title, url)
                .with_description(abstract_s)
                .with_source(source_label);
            if let Some(p) = published {
                hit = hit.with_published_at(p);
            }
            if let Some(d) = doi {
                hit = hit.with_extra("doi", serde_json::Value::String(d));
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
