// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::truncate_chars;
use async_trait::async_trait;

pub struct SemanticScholarEngine;

#[async_trait]
impl SearchEngine for SemanticScholarEngine {
    fn id(&self) -> &'static str {
        "semantic_scholar"
    }

    fn label(&self) -> &'static str {
        "Semantic Scholar"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(1, 30);
        let url = format!(
            "https://api.semanticscholar.org/graph/v1/paper/search?query={encoded}&limit={limit}&fields=title,abstract,authors,year,venue,externalIds,url"
        );
        let client = ctx.build_http_client()?;
        let mut req = client.get(&url).header("Accept", "application/json");
        if let Some(key) = ctx.api_keys.semantic_scholar.as_deref() {
            if !key.is_empty() {
                req = req.header("x-api-key", key);
            }
        }
        let response = req.send().await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Semantic Scholar search failed with status: {}",
                response.status()
            );
        }
        let json: serde_json::Value = response.json().await?;
        let mut hits = Vec::new();
        if let Some(data) = json.get("data").and_then(|v| v.as_array()) {
            for item in data.iter().take(ctx.limit) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if title.is_empty() {
                    continue;
                }
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        item.get("externalIds")
                            .and_then(|e| e.get("DOI"))
                            .and_then(|v| v.as_str())
                            .map(|d| format!("https://doi.org/{d}"))
                            .unwrap_or_default()
                    });
                if url.is_empty() {
                    continue;
                }
                let venue = item
                    .get("venue")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let year = item.get("year").and_then(|v| v.as_i64()).unwrap_or(0);
                let authors = item
                    .get("authors")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                            .take(4)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let abstract_text = item
                    .get("abstract")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut desc = String::new();
                if !authors.is_empty() {
                    desc.push_str(&format!("[{authors}] "));
                }
                if !venue.is_empty() {
                    desc.push_str(&format!("{venue}. "));
                }
                if year > 0 {
                    desc.push_str(&format!("({year}) "));
                }
                if !abstract_text.is_empty() {
                    desc.push_str(&abstract_text);
                }
                let mut hit = SearchHit::new(self.id(), title, url)
                    .with_description(truncate_chars(&desc, 320))
                    .with_source("Semantic Scholar");
                if year > 0 {
                    hit = hit.with_published_at(year.to_string());
                }
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}
