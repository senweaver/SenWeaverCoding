// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static KR36_PAYLOAD_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#""articleId":\s*"?(\d+)"_",\s*"widgetTitle":\s*"([^"]+)","widgetContent":\s*"([^"]*)""#,
        "kr36 payload regex",
    )
});

pub struct Kr36Engine;

#[async_trait]
impl SearchEngine for Kr36Engine {
    fn id(&self) -> &'static str {
        "kr36"
    }

    fn label(&self) -> &'static str {
        "36kr"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Lifestyle, SearchCategory::News, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let body = serde_json::json!({
            "partner_id": "wap",
            "param": {
                "searchWord": ctx.query,
                "siteId": 1,
                "platformId": 2,
                "sortField": "score",
                "pageSize": ctx.limit.clamp(5, 30),
                "pageCallback": "",
                "policeStatus": 0,
            },
            "timestamp": chrono::Utc::now().timestamp_millis(),
        });
        let client = ctx.build_http_client()?;
        let response = client
            .post("https://gateway.36kr.com/api/mis/nav/search/resultbytype")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("36kr search failed with status: {}", response.status());
        }
        let text = response.text().await?;
        let mut hits: Vec<SearchHit> = Vec::new();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            let item_list = json
                .get("data")
                .and_then(|v| v.get("itemList"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for item in item_list.iter().take(ctx.limit) {
                let article_id = item
                    .get("articleId")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let title = item
                    .get("widgetTitle")
                    .and_then(|v| v.as_str())
                    .map(clean_text)
                    .unwrap_or_default();
                if article_id.is_empty() || title.is_empty() {
                    continue;
                }
                let url = format!("https://36kr.com/p/{article_id}");
                let snippet = item
                    .get("widgetContent")
                    .and_then(|v| v.as_str())
                    .map(clean_text)
                    .unwrap_or_default();
                let author = item
                    .get("authorName")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let source_label = if !author.is_empty() {
                    format!("{author}  - 36kr")
                } else {
                    "36kr".to_string()
                };
                let published = item
                    .get("publishTime")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    });
                let mut hit = SearchHit::new(self.id(), title, url)
                    .with_description(snippet)
                    .with_source(source_label);
                if let Some(p) = published {
                    hit = hit.with_published_at(p);
                }
                hits.push(hit);
            }
        }
        if hits.is_empty() {
            for caps in KR36_PAYLOAD_RE.captures_iter(&text) {
                if hits.len() >= ctx.limit {
                    break;
                }
                let id = &caps[1];
                let title = clean_text(&caps[2]);
                let snippet = clean_text(&caps[3]);
                if title.is_empty() {
                    continue;
                }
                hits.push(
                    SearchHit::new(self.id(), title, format!("https://36kr.com/p/{id}"))
                        .with_description(snippet)
                        .with_source("36kr".to_string()),
                );
            }
        }
        if hits.is_empty() {
            anyhow::bail!("36kr returned no parseable results");
        }
        Ok(hits)
    }
}
