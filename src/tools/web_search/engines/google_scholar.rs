// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit, pick_rotating_user_agent};
use super::super::parsers::{clean_text, truncate_chars};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static GS_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div[^>]*class="[^"]*gs_r\s+gs_or\s+gs_scl[^"]*"[^>]*>([\s\S]*?)</div>\s*</div>\s*</div>"#)
        .expect("gs block regex")
});
static GS_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<h3[^>]*class="[^"]*gs_rt[^"]*"[^>]*>([\s\S]*?)</h3>"#).expect("gs title regex")
});
static GS_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<a[^>]*href="([^"]+)""#).expect("gs link regex"));
static GS_AUTHOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div[^>]*class="[^"]*gs_a[^"]*"[^>]*>([\s\S]*?)</div>"#)
        .expect("gs author regex")
});
static GS_SNIPPET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div[^>]*class="[^"]*gs_rs[^"]*"[^>]*>([\s\S]*?)</div>"#)
        .expect("gs snippet regex")
});
static GS_CITED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)Cited by\s*\d+"#).expect("gs cited regex")
});
static GS_RI_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div[^>]*class="[^"]*gs_ri[^"]*"[^>]*>([\s\S]*?)(?:</div>\s*<div[^>]*class="[^"]*gs_ri|</div>\s*</div>\s*</div>)"#)
        .expect("gs ri fallback regex")
});

pub struct GoogleScholarEngine;

#[async_trait]
impl SearchEngine for GoogleScholarEngine {
    fn id(&self) -> &'static str {
        "google_scholar"
    }

    fn label(&self) -> &'static str {
        "Google Scholar"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Academic]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let num = ctx.limit.clamp(1, 20);
        let url = format!("https://scholar.google.com/scholar?q={encoded}&hl=en&num={num}");
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header("User-Agent", pick_rotating_user_agent())
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await?;
        let status = response.status();
        let html = response.text().await.unwrap_or_default();
        if !status.is_success()
            || html.contains("Please show you're not a robot")
            || html.contains("/sorry/index")
        {
            anyhow::bail!("Google Scholar blocked or returned status {status}");
        }
        let mut hits = Vec::new();
        for block_caps in GS_BLOCK_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            let block = &block_caps[1];
            let Some(title_caps) = GS_TITLE_RE.captures(block) else {
                continue;
            };
            let title_inner = &title_caps[1];
            let title = strip_label_prefix(&clean_text(title_inner));
            if title.is_empty() {
                continue;
            }
            let url_str = GS_LINK_RE
                .captures(title_inner)
                .map(|c| c[1].to_string())
                .unwrap_or_else(|| {
                    format!(
                        "https://scholar.google.com/scholar?q={}",
                        urlencoding::encode(&title)
                    )
                });
            let author_info = GS_AUTHOR_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let snippet = GS_SNIPPET_RE
                .captures(block)
                .map(|c| clean_text(&c[1]))
                .unwrap_or_default();
            let cited_by = GS_CITED_RE
                .find(block)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let mut desc = String::new();
            if !author_info.is_empty() {
                desc.push_str(&format!("[{author_info}] "));
            }
            if !cited_by.is_empty() {
                desc.push_str(&format!("({cited_by}) "));
            }
            if !snippet.is_empty() {
                desc.push_str(&snippet);
            }
            hits.push(
                SearchHit::new(self.id(), title, url_str)
                    .with_description(truncate_chars(&desc, 320))
                    .with_source("Google Scholar"),
            );
        }
        if hits.is_empty() {
            for block_caps in GS_RI_BLOCK_RE.captures_iter(&html) {
                if hits.len() >= ctx.limit {
                    break;
                }
                let block = &block_caps[1];
                let Some(title_caps) = GS_TITLE_RE.captures(block) else {
                    continue;
                };
                let title_inner = &title_caps[1];
                let title = strip_label_prefix(&clean_text(title_inner));
                if title.is_empty() {
                    continue;
                }
                let url_str = GS_LINK_RE
                    .captures(title_inner)
                    .map(|c| c[1].to_string())
                    .unwrap_or_else(|| {
                        format!(
                            "https://scholar.google.com/scholar?q={}",
                            urlencoding::encode(&title)
                        )
                    });
                let author_info = GS_AUTHOR_RE
                    .captures(block)
                    .map(|c| clean_text(&c[1]))
                    .unwrap_or_default();
                let snippet = GS_SNIPPET_RE
                    .captures(block)
                    .map(|c| clean_text(&c[1]))
                    .unwrap_or_default();
                let mut desc = String::new();
                if !author_info.is_empty() {
                    desc.push_str(&format!("[{author_info}] "));
                }
                if !snippet.is_empty() {
                    desc.push_str(&snippet);
                }
                hits.push(
                    SearchHit::new(self.id(), title, url_str)
                        .with_description(truncate_chars(&desc, 320))
                        .with_source("Google Scholar"),
                );
            }
        }
        Ok(hits)
    }
}

fn strip_label_prefix(s: &str) -> String {
    if let Some(stripped) = s
        .trim_start()
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']').map(|(_, after)| after.trim()))
    {
        stripped.to_string()
    } else {
        s.to_string()
    }
}
