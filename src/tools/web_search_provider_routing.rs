// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSearchProviderRoute {
    DuckDuckGo,
    Brave,
    SearXNG,
    Tavily,
    Exa,
    Baidu,
}

impl WebSearchProviderRoute {
    pub fn label(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Brave => "Brave",
            Self::SearXNG => "SearXNG",
            Self::Tavily => "Tavily",
            Self::Exa => "Exa",
            Self::Baidu => "Baidu",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebSearchProviderResolution {
    pub route: WebSearchProviderRoute,
    pub canonical_provider: &'static str,
    pub used_fallback: bool,
}

pub const DEFAULT_WEB_SEARCH_PROVIDER: &str = "duckduckgo";
const BRAVE_PROVIDER: &str = "brave";
const SEARXNG_PROVIDER: &str = "searxng";
const TAVILY_PROVIDER: &str = "tavily";
const EXA_PROVIDER: &str = "exa";
const BAIDU_PROVIDER: &str = "baidu";

pub fn resolve_web_search_provider(raw_provider: &str) -> WebSearchProviderResolution {
    let normalized = raw_provider.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "default" | "duckduckgo" | "ddg" | "duck-duck-go" | "duck_duck_go" => {
            WebSearchProviderResolution {
                route: WebSearchProviderRoute::DuckDuckGo,
                canonical_provider: DEFAULT_WEB_SEARCH_PROVIDER,
                used_fallback: false,
            }
        }
        "brave" | "brave-search" | "brave_search" => WebSearchProviderResolution {
            route: WebSearchProviderRoute::Brave,
            canonical_provider: BRAVE_PROVIDER,
            used_fallback: false,
        },
        "searxng" | "searx" | "searx-ng" | "searx_ng" => WebSearchProviderResolution {
            route: WebSearchProviderRoute::SearXNG,
            canonical_provider: SEARXNG_PROVIDER,
            used_fallback: false,
        },
        "tavily" | "tavily-search" | "tavily_search" => WebSearchProviderResolution {
            route: WebSearchProviderRoute::Tavily,
            canonical_provider: TAVILY_PROVIDER,
            used_fallback: false,
        },
        "exa" | "exa-search" | "exa_search" => WebSearchProviderResolution {
            route: WebSearchProviderRoute::Exa,
            canonical_provider: EXA_PROVIDER,
            used_fallback: false,
        },
        "baidu" | "baidu-search" | "baidu_search" => WebSearchProviderResolution {
            route: WebSearchProviderRoute::Baidu,
            canonical_provider: BAIDU_PROVIDER,
            used_fallback: false,
        },
        _ => WebSearchProviderResolution {
            route: WebSearchProviderRoute::DuckDuckGo,
            canonical_provider: DEFAULT_WEB_SEARCH_PROVIDER,
            used_fallback: true,
        },
    }
}
