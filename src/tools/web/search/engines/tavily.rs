// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit, TimeRange};
use async_trait::async_trait;
use serde_json::json;

pub struct TavilyEngine;

#[async_trait]
impl SearchEngine for TavilyEngine {
    fn id(&self) -> &'static str {
        "tavily"
    }

    fn label(&self) -> &'static str {
        "Tavily"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::News, SearchCategory::Academic]
    }

    fn requires_api_key(&self) -> bool {
        true
    }

    fn is_available(&self, keys: &ApiKeys) -> bool {
        keys.tavily.as_ref().is_some_and(|k| !k.is_empty())
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let api_key = ctx
            .api_keys
            .tavily
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Tavily API key not configured"))?;
        let default_depth = if matches!(ctx.category, SearchCategory::Academic | SearchCategory::Code) {
            "advanced"
        } else {
            "basic"
        };
        let depth = ctx
            .extra_str("search_depth")
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| s == "basic" || s == "advanced")
            .unwrap_or_else(|| default_depth.to_string());
        let include_answer = ctx.extra_bool("include_answer").unwrap_or(false);
        let include_raw_content = ctx.extra_bool("include_raw_content").unwrap_or(false);
        let time_param = ctx.time_range.map(|t| match t {
            TimeRange::Day => "day",
            TimeRange::Week => "week",
            TimeRange::Month => "month",
            TimeRange::Year => "year",
        });
        let mut body = json!({
            "api_key": api_key,
            "query": ctx.query,
            "search_depth": depth,
            "max_results": ctx.limit.clamp(1, 20) as i64,
            "include_answer": include_answer,
            "include_raw_content": include_raw_content,
        });
        let body_obj = body
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("tavily: failed to construct request JSON body as object"))?;
        if let Some(t) = time_param {
            body_obj.insert("time_range".to_string(), json!(t));
        }
        if let Some(arr) = ctx.extra.get("include_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            if !domains.is_empty() {
                body_obj.insert("include_domains".to_string(), json!(domains));
            }
        }
        if let Some(arr) = ctx.extra.get("exclude_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            if !domains.is_empty() {
                body_obj.insert("exclude_domains".to_string(), json!(domains));
            }
        }
        let client = ctx.build_http_client()?;
        let response = client
            .post("https://api.tavily.com/search")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Tavily search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let mut hits = Vec::new();
        let tavily_answer = json
            .get("answer")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(results) = json.get("results").and_then(|v| v.as_array()) {
            for item in results.iter().take(ctx.limit) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if title.is_empty() || url.is_empty() {
                    continue;
                }
                let content = item
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let score = item.get("score").and_then(|v| v.as_f64()).map(|v| v as f32);
                let mut hit = SearchHit::new(self.id(), title, url)
                    .with_description(content)
                    .with_source("Tavily");
                if let Some(s) = score {
                    hit.score = Some(s);
                }
                if let Some(pub_at) = item.get("published_date").and_then(|v| v.as_str()) {
                    hit = hit.with_published_at(pub_at);
                }
                if let Some(answer) = tavily_answer.as_ref() {
                    if hits.is_empty() {
                        hit = hit.with_extra(
                            "tavily_answer",
                            serde_json::Value::String(answer.clone()),
                        );
                    }
                }
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}
