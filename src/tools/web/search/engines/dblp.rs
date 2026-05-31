// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::truncate_chars;
use async_trait::async_trait;

pub struct DblpEngine;

#[async_trait]
impl SearchEngine for DblpEngine {
    fn id(&self) -> &'static str {
        "dblp"
    }

    fn label(&self) -> &'static str {
        "DBLP"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic, SearchCategory::Code]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(1, 30);
        let url = format!(
            "https://dblp.org/search/publ/api?q={encoded}&format=json&h={limit}&c=0"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("DBLP search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let hits_arr = json
            .get("result")
            .and_then(|r| r.get("hits"))
            .and_then(|h| h.get("hit"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in hits_arr.iter().take(ctx.limit) {
            let info = item.get("info").cloned().unwrap_or_default();
            let title = info
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .trim_end_matches('.')
                .to_string();
            if title.is_empty() {
                continue;
            }
            let url = info
                .get("ee")
                .or_else(|| info.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if url.is_empty() {
                continue;
            }
            let authors = extract_authors(&info);
            let year = info
                .get("year")
                .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string())))
                .unwrap_or_default();
            let venue = info
                .get("venue")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let pub_type = info
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
            if !venue.is_empty() {
                desc.push_str(&format!("[{venue}] "));
            }
            if !pub_type.is_empty() {
                desc.push_str(&format!("({pub_type})"));
            }
            let mut hit = SearchHit::new(self.id(), title, url)
                .with_description(truncate_chars(desc.trim(), 320))
                .with_source("DBLP");
            if !year.is_empty() {
                hit = hit.with_published_at(year);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}

fn extract_authors(info: &serde_json::Value) -> String {
    let author_node = info.get("authors").and_then(|v| v.get("author"));
    let Some(node) = author_node else {
        return String::new();
    };
    let raw_list: Vec<&serde_json::Value> = if let Some(arr) = node.as_array() {
        arr.iter().collect()
    } else {
        vec![node]
    };
    let mut names: Vec<String> = Vec::new();
    for item in raw_list {
        if let Some(name) = item.as_str() {
            names.push(name.to_string());
        } else if let Some(name) = item.get("text").and_then(|v| v.as_str()) {
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return String::new();
    }
    let count = names.len();
    let mut joined = names.into_iter().take(3).collect::<Vec<_>>().join(", ");
    if count > 3 {
        joined.push_str(" et al.");
    }
    joined
}
