// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct HalEngine;

#[async_trait]
impl SearchEngine for HalEngine {
    fn id(&self) -> &'static str {
        "hal"
    }

    fn label(&self) -> &'static str {
        "HAL"
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
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(5, 30);
        let fields = "docid,uri_s,title_s,abstract_s,authFullName_s,producedDate_s,doiId_s,journalTitle_s";
        let url = format!(
            "https://api.archives-ouvertes.fr/search/?q={encoded}&rows={limit}&fl={fields}&wt=json"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("HAL search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let docs = json
            .get("response")
            .and_then(|v| v.get("docs"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for doc in docs.iter().take(ctx.limit) {
            let title = doc
                .get("title_s")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let uri = doc
                .get("uri_s")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if uri.is_empty() {
                continue;
            }
            let abstract_s = doc
                .get("abstract_s")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let authors = doc
                .get("authFullName_s")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .take(4)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let published = doc
                .get("producedDate_s")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let journal = doc
                .get("journalTitle_s")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let doi = doc
                .get("doiId_s")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let source_label = if !journal.is_empty() {
                if !authors.is_empty() {
                    format!("{authors}  - {journal}  - HAL")
                } else {
                    format!("{journal}  - HAL")
                }
            } else if !authors.is_empty() {
                format!("{authors}  - HAL")
            } else {
                "HAL".to_string()
            };
            let mut hit = SearchHit::new(self.id(), title, uri)
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
