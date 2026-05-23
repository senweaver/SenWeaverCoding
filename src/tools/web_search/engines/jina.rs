// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::truncate_chars;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static JINA_ENTRY_SPLIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n\[\d+\]\s+").expect("jina entry split regex"));

pub struct JinaEngine;

#[async_trait]
impl SearchEngine for JinaEngine {
    fn id(&self) -> &'static str {
        "jina"
    }

    fn label(&self) -> &'static str {
        "Jina"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::Academic, SearchCategory::News]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!("https://s.jina.ai/{encoded}");
        let client = ctx.build_http_client()?;
        let mut req = client
            .get(&url)
            .header("Accept", "text/plain")
            .header("X-With-Generated-Alt", "true");
        if let Some(key) = ctx.api_keys.jina.as_deref() {
            if !key.is_empty() {
                req = req.header("Authorization", format!("Bearer {key}"));
            }
        }
        let response = req.send().await?;
        if !response.status().is_success() {
            anyhow::bail!("Jina search failed with status: {}", response.status());
        }
        let text = response.text().await?;
        let mut hits = Vec::new();
        let entries: Vec<&str> = JINA_ENTRY_SPLIT_RE.split(&text).skip(1).collect();
        for entry in entries {
            if hits.len() >= ctx.limit {
                break;
            }
            let trimmed = entry.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            let mut lines = trimmed.lines();
            let title = lines.next().unwrap_or("").trim().to_string();
            if title.is_empty() {
                continue;
            }
            let mut url_str = String::new();
            let mut snippet_parts: Vec<&str> = Vec::new();
            let mut url_seen = false;
            for line in lines {
                if !url_seen {
                    if let Some(rest) = line.strip_prefix("URL: ") {
                        url_str = rest.trim().to_string();
                        url_seen = true;
                        continue;
                    }
                } else {
                    let trimmed_line = line.trim();
                    if trimmed_line.is_empty() {
                        continue;
                    }
                    snippet_parts.push(trimmed_line);
                }
            }
            if url_str.is_empty() {
                continue;
            }
            let snippet = truncate_chars(&snippet_parts.join(" "), 320);
            hits.push(
                SearchHit::new(self.id(), title, url_str).with_description(snippet),
            );
        }
        Ok(hits)
    }
}
