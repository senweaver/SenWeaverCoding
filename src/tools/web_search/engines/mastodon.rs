// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;

pub struct MastodonEngine;

#[async_trait]
impl SearchEngine for MastodonEngine {
    fn id(&self) -> &'static str {
        "mastodon"
    }

    fn label(&self) -> &'static str {
        "Mastodon"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Social]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let instance = ctx
            .api_keys
            .mastodon_instance
            .as_deref()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://mastodon.social".to_string());
        let encoded = urlencoding::encode(&ctx.query);
        let limit = ctx.limit.clamp(5, 40);
        let url = format!(
            "{instance}/api/v2/search?q={encoded}&type=statuses&limit={limit}&resolve=true"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Mastodon search failed with status: {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let statuses = json
            .get("statuses")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for status in statuses.iter().take(ctx.limit) {
            let target_url = status
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if target_url.is_empty() {
                continue;
            }
            let content_html = status.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let content = clean_text(content_html);
            if content.is_empty() {
                continue;
            }
            let title = if content.chars().count() > 80 {
                let truncated: String = content.chars().take(80).collect();
                format!("{truncated}…")
            } else {
                content.clone()
            };
            let snippet = if content.chars().count() > 240 {
                let truncated: String = content.chars().take(240).collect();
                format!("{truncated}…")
            } else {
                content
            };
            let acct = status
                .get("account")
                .and_then(|v| v.get("acct"))
                .and_then(|v| v.as_str())
                .map(|s| format!("@{s}"))
                .unwrap_or_default();
            let display_name = status
                .get("account")
                .and_then(|v| v.get("display_name"))
                .and_then(|v| v.as_str())
                .map(clean_text)
                .unwrap_or_default();
            let source_label = match (display_name.is_empty(), acct.is_empty()) {
                (false, false) => format!("{display_name} ({acct}) — Mastodon"),
                (false, true) => format!("{display_name} — Mastodon"),
                (true, false) => format!("{acct} — Mastodon"),
                _ => "Mastodon".to_string(),
            };
            let published = status
                .get("created_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut hit = SearchHit::new(self.id(), title, target_url)
                .with_description(snippet)
                .with_source(source_label);
            if let Some(p) = published {
                hit = hit.with_published_at(p);
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}
