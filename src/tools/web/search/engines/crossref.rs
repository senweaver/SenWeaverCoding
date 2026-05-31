// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::truncate_chars;
use async_trait::async_trait;

pub struct CrossrefEngine;

#[async_trait]
impl SearchEngine for CrossrefEngine {
    fn id(&self) -> &'static str {
        "crossref"
    }

    fn label(&self) -> &'static str {
        "Crossref"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let rows = ctx.limit.clamp(1, 25);
        let url = format!("https://api.crossref.org/works?query={encoded}&rows={rows}");
        let polite_email = ctx
            .api_keys
            .pubmed_email
            .clone()
            .filter(|e| !e.trim().is_empty());
        let ua = match polite_email.as_deref() {
            Some(email) => format!("ARS-SenWeaverCoding (mailto:{email})"),
            None => "ARS-SenWeaverCoding".to_string(),
        };
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .header("User-Agent", ua)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Crossref search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let items = json
            .get("message")
            .and_then(|m| m.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if title.is_empty() {
                continue;
            }
            let doi = item
                .get("DOI")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let url = if !doi.is_empty() {
                format!("https://doi.org/{doi}")
            } else {
                item.get("URL")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            if url.is_empty() {
                continue;
            }
            let authors = item
                .get("author")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    let mut names: Vec<String> = arr
                        .iter()
                        .filter_map(|a| {
                            let given = a.get("given").and_then(|v| v.as_str()).unwrap_or("");
                            let family = a.get("family").and_then(|v| v.as_str()).unwrap_or("");
                            let name = format!("{given} {family}").trim().to_string();
                            if name.is_empty() { None } else { Some(name) }
                        })
                        .collect();
                    let count = names.len();
                    names.truncate(3);
                    let mut joined = names.join(", ");
                    if count > 3 {
                        joined.push_str(" et al.");
                    }
                    joined
                })
                .unwrap_or_default();
            let year = extract_year(item);
            let container = item
                .get("container-title")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let publication_type = item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut desc = String::new();
            if !authors.is_empty() {
                desc.push_str(&format!("[{authors}] "));
            }
            if !year.is_empty() {
                desc.push_str(&format!("({year}) "));
            }
            if !container.is_empty() {
                desc.push_str(&format!("[{container}] "));
            }
            if !publication_type.is_empty() {
                desc.push_str(&format!("({publication_type})"));
            }
            let mut hit = SearchHit::new(self.id(), title, url)
                .with_description(truncate_chars(desc.trim(), 320))
                .with_source("Crossref");
            if !doi.is_empty() {
                hit = hit.with_extra("doi", serde_json::Value::String(doi));
            }
            if !year.is_empty() {
                hit = hit.with_published_at(year);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}

fn extract_year(item: &serde_json::Value) -> String {
    for key in ["issued", "published-print", "published-online", "created"] {
        let parts = item
            .get(key)
            .and_then(|v| v.get("date-parts"))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_array());
        if let Some(p) = parts {
            if let Some(year) = p.first().and_then(|v| v.as_i64()) {
                return year.to_string();
            }
        }
    }
    String::new()
}
