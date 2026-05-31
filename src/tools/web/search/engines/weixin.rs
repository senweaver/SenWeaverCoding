// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::super::parsers::clean_text;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

static WX_LI_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<li[^>]*(?:id="sogou_vr_[\w-]+"|class="[^"]*(?:results|news-list)[^"]*")([\s\S]+?)</li>"#,
        "weixin li regex",
    )
});
static WX_NEWSLIST_LI_FALLBACK_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<ul[^>]*class="[^"]*news-list[^"]*([\s\S]+?)</ul>"#,
        "weixin news-list scope regex",
    )
});
static WX_LI_PLAIN_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(r"</li>(.+?)</li>", "weixin plain li regex")
});
static WX_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<h3[^>]*>[\s\S]*?<a[^>]*href="([^"]+)(.+?)</a>"#,
        "weixin link regex",
    )
});
static WX_SNIPPET_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<p[^>]*class="[^"]*txt-info[^"]*(.+?)</p>"#,
        "weixin snippet regex",
    )
});
static WX_ACCOUNT_RE: LazyLock<Regex> = LazyLock::new(|| {
    crate::tools::web::search::engines::compile_regex(
        r#"<a[^>]*class="[^"]*account[^"]*(.+?)</a>"#,
        "weixin account regex",
    )
});

pub struct WeixinEngine;

#[async_trait]
impl SearchEngine for WeixinEngine {
    fn id(&self) -> &'static str {
        "weixin"
    }

    fn label(&self) -> &'static str {
        "Weixin"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Cn, SearchCategory::Social]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let encoded = urlencoding::encode(&ctx.query);
        let url = format!(
            "https://weixin.sogou.com/weixin?type=2&page=1&ie=utf8&query={encoded}"
        );
        let client = ctx.build_http_client()?;
        let response = client
            .get(&url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Referer", "https://weixin.sogou.com/")
            .header("Cookie", "ABTEST=0|1700000000|v1")
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Weixin/Sogou search failed with status: {}", response.status());
        }
        let html = response.text().await?;
        if html.contains("antispider") || html.contains("您的访问出错") {
            anyhow::bail!("Weixin/Sogou blocked the request (antispider)");
        }
        let mut hits = Vec::new();
        for li_caps in WX_LI_RE.captures_iter(&html) {
            if hits.len() >= ctx.limit {
                break;
            }
            extract_weixin_li(&li_caps[1], self.id(), &mut hits);
        }
        if hits.is_empty() {
            if let Some(scope_caps) = WX_NEWSLIST_LI_FALLBACK_RE.captures(&html) {
                let scope = &scope_caps[1];
                for li_caps in WX_LI_PLAIN_RE.captures_iter(scope) {
                    if hits.len() >= ctx.limit {
                        break;
                    }
                    extract_weixin_li(&li_caps[1], self.id(), &mut hits);
                }
            }
        }
        Ok(hits)
    }
}

fn extract_weixin_li(li: &str, engine_id: &str, hits: &mut Vec<SearchHit>) {
    let Some(link_caps) = WX_LINK_RE.captures(li) else {
        return;
    };
    let mut url_str = link_caps[1].to_string();
    let title = clean_text(&link_caps[2])
        .replace("red_beg", "")
        .replace("red_end", "");
    if title.is_empty() {
        return;
    }
    if !url_str.starts_with("http") {
        url_str = format!("https://weixin.sogou.com{url_str}");
    }
    let abstract_text = WX_SNIPPET_RE
        .captures(li)
        .map(|c| clean_text(&c[1]))
        .map(|s| s.replace("red_beg", "").replace("red_end", ""))
        .unwrap_or_default();
    let gzh_name = WX_ACCOUNT_RE
        .captures(li)
        .map(|c| clean_text(&c[1]))
        .unwrap_or_default();
    let description = if gzh_name.is_empty() {
        abstract_text
    } else {
        format!("[{gzh_name}] {abstract_text}")
    };
    let mut hit = SearchHit::new(engine_id, title, url_str).with_description(description);
    if !gzh_name.is_empty() {
        hit = hit.with_source(gzh_name);
    }
    hits.push(hit);
}
