// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::{clean_text, collapse_whitespace, strip_tags, truncate_chars};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static BAIDU_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<h3[^>]*class="[^"]*c-title[^"]*"[^>]*>[\s\S]{0,800}?<a[^>]*href="([^"]+)(.+?)</a>"#,
        "baidu title regex",
    )
});
static BAIDU_ABSTRACT_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<(?:span|div|p)[^>]*class="[^"]*(?:c-abstract|content-right_[\w-]*|c-span-last|c-color-text|c-gap-top-small|cu-line-clamp[\w-]*|line-clamp[\w-]*|c-row[\w-]*|cos[a-zA-Z0-9_-]*)[^"]*(.+?)</(?:span|div|p)>"#,
        "baidu abstract regex",
    )
});
static BAIDU_BLOCK_CLEANUP_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r"<(?:script|style|a|button|input|select|noscript)\b[^>]*>[\s\S]*?</(?:script|style|a|button|input|select|noscript)>",
        "baidu block cleanup regex",
    )
});
static BAIDU_BLOCK_START_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]+class="[^"]*\b(?:result|c-container)\b[^"]*""#,
        "baidu block start regex",
    )
});
static BAIDU_END_MARKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<div[^>]+id="(?:page|content_right|content_left_bottom)""#,
        "baidu end marker regex",
    )
});

pub struct BaiduEngine;

#[async_trait]
impl SearchEngine for BaiduEngine {
    fn id(&self) -> &'static str {
        "baidu"
    }

    fn label(&self) -> &'static str {
        "Baidu"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web, SearchCategory::Cn]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let rn = ctx.limit.clamp(5, 10);
        let url = format!("https://www.baidu.com/s?wd={encoded}&rn={rn}&ie=utf-8");

        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Baidu search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        if html.contains("百度安全验证") || html.contains("请输入验证码") {
            anyhow::bail!("Baidu blocked the request with a captcha challenge");
        }

        let mut entries: Vec<SearchHit> = Vec::new();
        if let Some(blocks) = collect_baidu_blocks(&html) {
            for block_html in blocks {
                if entries.len() >= ctx.limit {
                    break;
                }
                let Some(t_caps) = BAIDU_TITLE_RE.captures(block_html) else {
                    continue;
                };
                let url_str = t_caps[1].trim().to_string();
                let title = clean_text(&strip_tags(&t_caps[2]));
                if title.is_empty() || url_str.is_empty() {
                    continue;
                }
                let snippet = extract_snippet(block_html, &title);
                entries.push(
                    SearchHit::new(self.id(), title, url_str).with_description(snippet),
                );
            }
        }

        if entries.is_empty() {
            let title_matches: Vec<_> = BAIDU_TITLE_RE
                .captures_iter(&html)
                .take(ctx.limit + 2)
                .collect();
            let abstract_matches: Vec<_> = BAIDU_ABSTRACT_RE
                .captures_iter(&html)
                .take(ctx.limit + 4)
                .collect();
            for (i, caps) in title_matches.iter().enumerate().take(ctx.limit) {
                let url_str = caps[1].trim().to_string();
                let title = clean_text(&strip_tags(&caps[2]));
                if title.is_empty() || url_str.is_empty() {
                    continue;
                }
                let snippet = abstract_matches
                    .get(i)
                    .map(|c| clean_text(&c[1]))
                    .unwrap_or_default();
                entries.push(
                    SearchHit::new(self.id(), title, url_str).with_description(snippet),
                );
            }
        }
        Ok(entries)
    }
}

fn collect_baidu_blocks(html: &str) -> Option<Vec<&str>> {
    let mut starts: Vec<usize> = BAIDU_BLOCK_START_RE
        .find_iter(html)
        .map(|m| m.start())
        .collect();
    if starts.is_empty() {
        return None;
    }
    let end_pos = BAIDU_END_MARKER_RE
        .find(html)
        .map(|m| m.start())
        .unwrap_or(html.len());
    starts.push(end_pos);
    let mut blocks = Vec::with_capacity(starts.len().saturating_sub(1));
    for window in starts.windows(2) {
        let (s, e) = (window[0], window[1]);
        if e > s && e <= html.len() {
            blocks.push(&html[s..e]);
        }
    }
    Some(blocks)
}

fn extract_snippet(block_html: &str, title: &str) -> String {
    if let Some(caps) = BAIDU_ABSTRACT_RE.captures(block_html) {
        let s = collapse_whitespace(&strip_tags(&caps[1]));
        if s.chars().count() >= 8 {
            return truncate_chars(&s, 320);
        }
    }
    let cleaned = BAIDU_BLOCK_CLEANUP_RE.replace_all(block_html, " ");
    let stripped = strip_tags(&cleaned);
    let collapsed = collapse_whitespace(&stripped);
    let title_collapsed = collapse_whitespace(title);
    let body = if !title_collapsed.is_empty() {
        collapsed.replacen(&title_collapsed, " ", 1)
    } else {
        collapsed
    };
    let mut filtered = body;
    for noise in [
        "百度快照",
        "查看更多",
        "点击查看",
        "更多 >",
        "更多>",
        "评论",
        "分享",
        "收藏",
        "投诉",
        "举报",
    ] {
        filtered = filtered.replace(noise, " ");
    }
    let filtered = collapse_whitespace(&filtered);
    let trimmed = filtered.trim();
    if trimmed.chars().count() < 12 {
        return String::new();
    }
    truncate_chars(trimmed, 320)
}
