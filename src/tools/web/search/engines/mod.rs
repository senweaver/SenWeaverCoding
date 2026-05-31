// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod baidu;
pub mod brave;
pub mod duckduckgo;
pub mod github;
pub mod searxng;

#[cfg(feature = "tool-search-broad")]
pub mod arxiv;
#[cfg(feature = "tool-search-broad")]
pub mod bilibili;
#[cfg(feature = "tool-search-broad")]
pub mod bing;
#[cfg(feature = "tool-search-broad")]
pub mod biorxiv;
#[cfg(feature = "tool-search-broad")]
pub mod core_engine;
#[cfg(feature = "tool-search-broad")]
pub mod crossref;
#[cfg(feature = "tool-search-broad")]
pub mod csdn;
#[cfg(feature = "tool-search-broad")]
pub mod dblp;
#[cfg(feature = "tool-search-broad")]
pub mod devto;
#[cfg(feature = "tool-search-broad")]
pub mod exa;
#[cfg(feature = "tool-search-broad")]
pub mod gitee;
#[cfg(feature = "tool-search-broad")]
pub mod gitlab;
#[cfg(feature = "tool-search-broad")]
pub mod google;
#[cfg(feature = "tool-search-broad")]
pub mod hackernews;
#[cfg(feature = "tool-search-broad")]
pub mod hal;
#[cfg(feature = "tool-search-broad")]
pub mod ieee_xplore;
#[cfg(feature = "tool-search-broad")]
pub mod invidious;
#[cfg(feature = "tool-search-broad")]
pub mod ithome;
#[cfg(feature = "tool-search-broad")]
pub mod jina;
#[cfg(feature = "tool-search-broad")]
pub mod juejin;
#[cfg(feature = "tool-search-broad")]
pub mod kr36;
#[cfg(feature = "tool-search-broad")]
pub mod mastodon;
#[cfg(feature = "tool-search-broad")]
pub mod openalex;
#[cfg(feature = "tool-search-broad")]
pub mod pubmed;
#[cfg(feature = "tool-search-broad")]
pub mod reddit;
#[cfg(feature = "tool-search-broad")]
pub mod segmentfault;
#[cfg(feature = "tool-search-broad")]
pub mod semantic_scholar;
#[cfg(feature = "tool-search-broad")]
pub mod serper;
#[cfg(feature = "tool-search-broad")]
pub mod sohu;
#[cfg(feature = "tool-search-broad")]
pub mod sspai;
#[cfg(feature = "tool-search-broad")]
pub mod ssrn;
#[cfg(feature = "tool-search-broad")]
pub mod stackoverflow;
#[cfg(feature = "tool-search-broad")]
pub mod tavily;
#[cfg(feature = "tool-search-broad")]
pub mod thepaper;
#[cfg(feature = "tool-search-broad")]
pub mod v2ex;
#[cfg(feature = "tool-search-broad")]
pub mod weibo;
#[cfg(feature = "tool-search-broad")]
pub mod weixin;
#[cfg(feature = "tool-search-broad")]
pub mod wikipedia;
#[cfg(feature = "tool-search-broad")]
pub mod yahoo_news;
#[cfg(feature = "tool-search-broad")]
pub mod zhihu;

pub(crate) fn compile_regex(pattern: &str, name: &str) -> regex::Regex {
    match regex::Regex::new(pattern) {
        Ok(re) => re,
        Err(e) => {
            tracing::warn!(
                target: "web.search.engines",
                regex = name,
                "failed to compile search engine regex ({e}); matcher disabled (no matches)"
            );
            regex::Regex::new(r"[^\s\S]").expect("never-match regex must compile")
        }
    }
}
