// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static BRAVE_SNIPPET_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]*class="[^"]*snippet[^"]*(.+?)</div>\s*</div>"#,
        "brave snippet block regex",
    )
});
static BRAVE_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<a[^>]*class="[^"]*heading-serpresult[^"]*"[^>]*href="([^"]+)(.+?)</a>"#,
        "brave link regex",
    )
});
static BRAVE_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]*class="[^"]*title[^"]*(.+?)</div>"#,
        "brave title regex",
    )
});
static BRAVE_DESC_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]*class="[^"]*snippet-description[^"]*(.+?)</div>"#,
        "brave snippet description regex",
    )
});

pub struct BraveEngine;

#[async_trait]
impl SearchEngine for BraveEngine {
    fn id(&self) -> &'static str {
        "brave"
    }

    fn label(&self) -> &'static str {
        "Brave"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::News]
    }

    fn requires_api_key(&self) -> bool {
        false
    }

    fn is_available(&self, _keys: &ApiKeys) -> bool {
        true
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        if let Some(api_key) = ctx
            .api_keys
            .brave
            .as_ref()
            .filter(|k| !k.trim().is_empty())
            .cloned()
        {
            match search_with_api(ctx, &api_key).await {
                Ok(hits) if !hits.is_empty() => return Ok(hits),
                Ok(_) => {}
                Err(err) => {
                    tracing::debug!(
                        target: "tools.web_search.brave",
                        error = %err,
                        "Brave API failed; falling back to HTML scrape"
                    );
                }
            }
        }
        search_with_html(ctx).await
    }
}

async fn search_with_api(ctx: &SearchContext, api_key: &str) -> anyhow::Result<Vec<SearchHit>> {
    let encoded = urlencoding::encode(&ctx.query);
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={encoded}&count={}",
        ctx.limit
    );
    let client = ctx.build_http_client()?;
    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("Brave API failed with status: {}", response.status());
    }
    let json: serde_json::Value = response.json().await?;
    let results = json
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array())
        .ok_or_else(|| anyhow::anyhow!("Invalid Brave API response"))?;
    let mut hits = Vec::new();
    for result in results.iter().take(ctx.limit) {
        let title = result
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let url = result
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        let description = result
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        if title.is_empty() && url.is_empty() {
            continue;
        }
        hits.push(SearchHit::new("brave", title, url).with_description(description));
    }
    Ok(hits)
}

async fn search_with_html(ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
    let encoded = urlencoding::encode(&ctx.query);
    let url = format!("https://search.brave.com/search?q={encoded}&source=web");
    let client = ctx.build_http_client()?;
    let response = client
        .get(&url)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("Accept-Language", "en-US,en;q=0.9,zh-CN;q=0.7")
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("Brave HTML search failed with status: {}", response.status());
    }
    let html = response.text().await?;
    if html.contains("captcha") || html.contains("verify-challenge") {
        anyhow::bail!("Brave HTML search blocked by captcha challenge");
    }
    let mut hits = Vec::new();
    for snippet_caps in BRAVE_SNIPPET_RE.captures_iter(&html) {
        if hits.len() >= ctx.limit {
            break;
        }
        let block = &snippet_caps[1];
        let Some(link_caps) = BRAVE_LINK_RE.captures(block) else {
            continue;
        };
        let url_str = link_caps[1].trim().to_string();
        if !url_str.starts_with("http") {
            continue;
        }
        let title = BRAVE_TITLE_RE
            .captures(block)
            .map(|c| clean_text(&c[1]))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| clean_text(&link_caps[2]));
        if title.is_empty() {
            continue;
        }
        let description = BRAVE_DESC_RE
            .captures(block)
            .map(|c| clean_text(&c[1]))
            .unwrap_or_default();
        hits.push(SearchHit::new("brave", title, url_str).with_description(description));
    }
    Ok(hits)
}
