// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct DuckDuckGoImagesEngine;

#[async_trait]
impl SearchEngine for DuckDuckGoImagesEngine {
    fn id(&self) -> &'static str {
        "duckduckgo_images"
    }

    fn label(&self) -> &'static str {
        "DuckDuckGo Images"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Image]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let client = ctx.build_http_client()?;
        let vqd = fetch_vqd(&client, &ctx.query).await?;
        let size_filter = ctx
            .extra_str("size")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let type_filter = ctx
            .extra_str("image_type")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let mut url = format!(
            "https://duckduckgo.com/i.js?q={}&o=json&p=1&s=0&vqd={}",
            urlencoding::encode(&ctx.query),
            urlencoding::encode(&vqd)
        );
        if !size_filter.is_empty() {
            url.push_str(&format!("&iaf=size:{}", urlencoding::encode(&size_filter)));
        }
        if !type_filter.is_empty() {
            url.push_str(&format!("&iaf=type:{}", urlencoding::encode(&type_filter)));
        }
        let resp = client
            .get(&url)
            .header("Accept", "application/json, text/javascript, */*; q=0.01")
            .header("Referer", "https://duckduckgo.com/")
            .header("X-Requested-With", "XMLHttpRequest")
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "DuckDuckGo images failed with status: {}",
                resp.status()
            );
        }
        let body = resp.text().await?;
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse DuckDuckGo images JSON: {e}"))?;
        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in results.iter().take(ctx.limit) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let image_url = item
                .get("image")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let thumbnail = item
                .get("thumbnail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let source = item
                .get("source")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let url_field = item
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let width = item.get("width").and_then(|v| v.as_u64()).unwrap_or(0);
            let height = item.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
            let primary_link = if !url_field.is_empty() {
                url_field
            } else if !image_url.is_empty() {
                image_url.clone()
            } else {
                continue;
            };
            let snippet = if width > 0 && height > 0 {
                format!("{width}×{height}")
            } else {
                String::new()
            };
            let mut hit = SearchHit::new(self.id(), if title.is_empty() { "(image)".to_string() } else { title }, primary_link)
                .with_description(snippet)
                .with_source(if source.is_empty() {
                    "DuckDuckGo Images".to_string()
                } else {
                    format!("{source} — DuckDuckGo Images")
                });
            if !image_url.is_empty() {
                hit = hit.with_extra("image_url", serde_json::Value::String(image_url));
            }
            if !thumbnail.is_empty() {
                hit = hit.with_extra("thumbnail", serde_json::Value::String(thumbnail));
            }
            if width > 0 && height > 0 {
                hit = hit
                    .with_extra("width", serde_json::Value::Number(width.into()))
                    .with_extra("height", serde_json::Value::Number(height.into()));
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}

async fn fetch_vqd(client: &reqwest::Client, query: &str) -> anyhow::Result<String> {
    let url = format!("https://duckduckgo.com/?q={}", urlencoding::encode(query));
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "DuckDuckGo vqd lookup failed with status: {}",
            resp.status()
        );
    }
    let body = resp.text().await?;
    if let Some(pos) = body.find("vqd='") {
        let start = pos + 5;
        if let Some(end) = body[start..].find('\'') {
            return Ok(body[start..start + end].to_string());
        }
    }
    if let Some(pos) = body.find("vqd=\"") {
        let start = pos + 5;
        if let Some(end) = body[start..].find('"') {
            return Ok(body[start..start + end].to_string());
        }
    }
    if let Some(pos) = body.find("vqd=") {
        let start = pos + 4;
        let end = body[start..]
            .find(|c: char| !c.is_alphanumeric() && c != '-')
            .unwrap_or(body.len() - start);
        let token = &body[start..start + end];
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }
    anyhow::bail!("Could not extract vqd token from DuckDuckGo")
}
