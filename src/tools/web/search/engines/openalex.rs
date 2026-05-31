// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::truncate_chars;
use async_trait::async_trait;

pub struct OpenAlexEngine;

#[async_trait]
impl SearchEngine for OpenAlexEngine {
    fn id(&self) -> &'static str {
        "openalex"
    }

    fn label(&self) -> &'static str {
        "OpenAlex"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let per_page = ctx.limit.clamp(1, 25);
        let mailto = ctx
            .api_keys
            .pubmed_email
            .as_deref()
            .filter(|e| !e.trim().is_empty());
        let mailto_param = mailto
            .map(|m| format!("&mailto={}", urlencoding::encode(m)))
            .unwrap_or_default();
        let select = "id,title,authorships,publication_year,doi,primary_location,host_venue,cited_by_count";
        let url = format!(
            "https://api.openalex.org/works?search={encoded}&per-page={per_page}&select={select}{mailto_param}"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("OpenAlex search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let results = json
            .get("results")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in results.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if title.is_empty() {
                continue;
            }
            let doi = item
                .get("doi")
                .and_then(|v| v.as_str())
                .map(|s| s.trim_start_matches("https://doi.org/").to_string());
            let primary_url = item
                .get("primary_location")
                .and_then(|p| p.get("landing_page_url"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let id_url = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let url = primary_url
                .or_else(|| doi.as_deref().map(|d| format!("https://doi.org/{d}")))
                .or(id_url)
                .unwrap_or_default();
            if url.is_empty() {
                continue;
            }
            let authors = item
                .get("authorships")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    let mut names: Vec<String> = arr
                        .iter()
                        .filter_map(|a| {
                            a.get("author")
                                .and_then(|au| au.get("display_name"))
                                .and_then(|n| n.as_str().map(str::to_string))
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
            let year = item
                .get("publication_year")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let cited = item
                .get("cited_by_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let venue = item
                .get("host_venue")
                .and_then(|v| v.get("display_name"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    item.get("primary_location")
                        .and_then(|p| p.get("source"))
                        .and_then(|s| s.get("display_name"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("")
                .to_string();
            let mut desc = String::new();
            if !authors.is_empty() {
                desc.push_str(&format!("[{authors}] "));
            }
            if year > 0 {
                desc.push_str(&format!("({year}) "));
            }
            if !venue.is_empty() {
                desc.push_str(&format!("[{venue}] "));
            }
            if cited > 0 {
                desc.push_str(&format!("(cited by {cited})"));
            }
            let mut hit = SearchHit::new(self.id(), title, url)
                .with_description(truncate_chars(desc.trim(), 320))
                .with_source("OpenAlex");
            if let Some(d) = doi {
                hit = hit.with_extra("doi", serde_json::Value::String(d));
            }
            if year > 0 {
                hit = hit.with_published_at(year.to_string());
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
