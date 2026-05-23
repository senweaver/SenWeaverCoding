// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct IeeeXploreEngine;

#[async_trait]
impl SearchEngine for IeeeXploreEngine {
    fn id(&self) -> &'static str {
        "ieee_xplore"
    }

    fn label(&self) -> &'static str {
        "IEEE Xplore"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let body = serde_json::json!({
            "newsearch": true,
            "queryText": ctx.query,
            "highlight": false,
            "returnType": "SEARCH",
            "matchPubs": true,
            "rowsPerPage": ctx.limit.clamp(5, 25),
            "pageNumber": 1,
        });
        let client = ctx.build_http_client()?;
        let response = client
            .post("https://ieeexplore.ieee.org/rest/search")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", "https://ieeexplore.ieee.org")
            .header("Referer", "https://ieeexplore.ieee.org/search/searchresult.jsp")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "IEEE Xplore search failed with status: {}",
                response.status()
            );
        }
        let json: serde_json::Value = response.json().await?;
        let records = json
            .get("records")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for rec in records.iter().take(ctx.limit) {
            let title = rec
                .get("articleTitle")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let html_link = rec
                .get("htmlLink")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let document_link = rec
                .get("documentLink")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let url_path = if !html_link.is_empty() {
                html_link
            } else {
                document_link
            };
            if url_path.is_empty() {
                continue;
            }
            let url = if url_path.starts_with("http") {
                url_path
            } else {
                format!("https://ieeexplore.ieee.org{url_path}")
            };
            let abstract_s = rec
                .get("abstract")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let pub_year = rec
                .get("publicationYear")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    rec.get("publicationDate")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
            let authors = rec
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| {
                            a.get("preferredName")
                                .or_else(|| a.get("normalizedName"))
                                .and_then(|n| n.as_str())
                        })
                        .take(4)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let pub_title = rec
                .get("publicationTitle")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let source_label = match (authors.is_empty(), pub_title.is_empty()) {
                (false, false) => format!("{authors} — {pub_title} — IEEE Xplore"),
                (false, true) => format!("{authors} — IEEE Xplore"),
                (true, false) => format!("{pub_title} — IEEE Xplore"),
                _ => "IEEE Xplore".to_string(),
            };
            let doi = rec
                .get("doi")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut hit = SearchHit::new(self.id(), title, url)
                .with_description(abstract_s)
                .with_source(source_label);
            if let Some(p) = pub_year {
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
