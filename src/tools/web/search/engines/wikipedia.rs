// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct WikipediaEngine;

fn locale_to_wiki_lang(locale: Option<&str>) -> &'static str {
    let lc = locale.unwrap_or("").to_ascii_lowercase();
    if lc.starts_with("zh") {
        "zh"
    } else if lc.starts_with("ja") {
        "ja"
    } else if lc.starts_with("ko") {
        "ko"
    } else if lc.starts_with("fr") {
        "fr"
    } else if lc.starts_with("de") {
        "de"
    } else if lc.starts_with("es") {
        "es"
    } else if lc.starts_with("ru") {
        "ru"
    } else {
        "en"
    }
}

fn query_is_cjk(query: &str) -> bool {
    query.chars().any(|c| {
        matches!(c,
            '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{20000}'..='\u{2A6DF}'
        )
    })
}

#[async_trait]
impl SearchEngine for WikipediaEngine {
    fn id(&self) -> &'static str {
        "wikipedia"
    }

    fn label(&self) -> &'static str {
        "Wikipedia"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Wiki, SearchCategory::Web]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let lang = if ctx.locale.is_some() {
            locale_to_wiki_lang(ctx.locale.as_deref())
        } else if query_is_cjk(&ctx.query) {
            "zh"
        } else {
            "en"
        };
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(5, 30);
        let url = format!(
            "https://{lang}.wikipedia.org/w/api.php?action=query&list=search&srsearch={encoded}&srlimit={limit}&format=json&utf8=1&srprop=snippet|titlesnippet|timestamp"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                "SenWeaverCoding/0.1 (web_search; +https://senweaver.coding)",
            )
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Wikipedia search failed with status: {}",
                response.status()
            );
        }
        let json: serde_json::Value = response.json().await?;
        let items = json
            .get("query")
            .and_then(|v| v.get("search"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let snippet = item
                .get("snippet")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let title_underscored = title.replace(' ', "_");
            let encoded_title = urlencoding::encode(&title_underscored);
            let target_url = format!("https://{lang}.wikipedia.org/wiki/{encoded_title}");
            let timestamp = item
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut hit = SearchHit::new(self.id(), title, target_url)
                .with_description(snippet)
                .with_source(format!("Wikipedia ({lang})"));
            if let Some(t) = timestamp {
                hit = hit.with_published_at(t);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
