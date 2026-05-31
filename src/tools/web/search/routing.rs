// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::engine::{ApiKeys, SearchCategory, SearchEngine};
use std::sync::{Arc, OnceLock};

pub struct EngineRegistry {
    engines: Vec<Arc<dyn SearchEngine>>,
}

impl EngineRegistry {
    pub fn all(&self) -> &[Arc<dyn SearchEngine>] {
        &self.engines
    }

    pub fn find(&self, id: &str) -> Option<Arc<dyn SearchEngine>> {
        let id_lc = id.to_ascii_lowercase();
        let normalized = canonical_alias(&id_lc).unwrap_or(id_lc.as_str());
        self.engines
            .iter()
            .find(|e| e.id().eq_ignore_ascii_case(normalized))
            .cloned()
    }

    pub fn fallback_chain(
        &self,
        category: SearchCategory,
        keys: &ApiKeys,
        preferred: Option<&str>,
        query: &str,
    ) -> Vec<Arc<dyn SearchEngine>> {
        let mut chain: Vec<Arc<dyn SearchEngine>> = Vec::new();
        let push_unique = |engine: Arc<dyn SearchEngine>, sink: &mut Vec<Arc<dyn SearchEngine>>| {
            if !sink.iter().any(|e| e.id() == engine.id()) {
                sink.push(engine);
            }
        };
        if let Some(pref) = preferred.and_then(|p| self.find(p)) {
            push_unique(pref, &mut chain);
        }
        let cjk = query_has_cjk(query);

        if cjk {
            for fallback_id in cn_priority_ids() {
                if let Some(engine) = self.find(fallback_id) {
                    if engine.is_available(keys) {
                        push_unique(engine, &mut chain);
                    }
                }
            }
        }
        for fallback_id in default_chain_for(category) {
            if let Some(engine) = self.find(fallback_id) {
                if engine.is_available(keys) {
                    push_unique(engine, &mut chain);
                }
            }
        }
        for engine in &self.engines {
            if engine.categories().contains(&category) && engine.is_available(keys) {
                push_unique(engine.clone(), &mut chain);
            }
        }
        for engine in &self.engines {
            if engine.is_available(keys) {
                push_unique(engine.clone(), &mut chain);
            }
        }
        chain
    }
}

fn query_has_cjk(query: &str) -> bool {
    query.chars().any(|c| {
        matches!(c,
            '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{20000}'..='\u{2A6DF}'
            | '\u{2A700}'..='\u{2B73F}'
            | '\u{F900}'..='\u{FAFF}'
        )
    })
}

fn cn_priority_ids() -> &'static [&'static str] {
    &[
        "baidu",
        "bing",
        "jina",
        "csdn",
        "juejin",
        "weixin",
        "zhihu",
    ]
}

fn canonical_alias(id: &str) -> Option<&'static str> {
    Some(match id {
        "ddg" | "duckduckgo" => "duckduckgo",
        "brave" => "brave",
        "searxng" | "searx" => "searxng",
        "baidu" => "baidu",
        "bing" | "ms-bing" => "bing",
        "bing_news" | "bing-news" | "bingnews" => "bing_news",
        "jina" | "s.jina" => "jina",
        "scholar" | "google_scholar" | "google-scholar" | "gscholar" => "google_scholar",
        "google_news" | "google-news" | "gnews" => "google_news",
        "yahoo_news" | "yahoo-news" | "yahoonews" => "yahoo_news",
        "thepaper" | "paper" | "pengpai" | "婢庢箖" => "thepaper",
        "sohu" | "鎼滅嫄" => "sohu",
        "pubmed" | "ncbi" => "pubmed",
        "arxiv" => "arxiv",
        "biorxiv" | "bioRxiv" | "bio-rxiv" => "biorxiv",
        "ssrn" => "ssrn",
        "ieee" | "ieee_xplore" | "ieeexplore" | "xplore" => "ieee_xplore",
        "hal" | "hal_open" => "hal",
        "core" | "core_ac" | "core_uk" => "core",
        "csdn" => "csdn",
        "juejin" => "juejin",
        "weixin" | "wechat" | "sogou" | "weixin_sogou" => "weixin",
        "serper" | "google-serper" => "serper",
        "tavily" => "tavily",
        "exa" => "exa",
        "github" | "github_code" | "gh" | "github_repos" | "github_repositories" => "github",
        "github_code_search" | "github_codes" | "github_code_files" | "gh_code" => "github_code_search",
        "github_issues" | "github_issue" | "gh_issues" | "gh_issue" | "github_prs" | "github_pr" => "github_issues",
        "github_users" | "github_user" | "gh_users" => "github_users",
        "github_advanced" | "github_adv" | "github_advanced_search" | "gh_advanced" => "github_advanced",
        "gitlab" | "gl" => "gitlab",
        "gitee" => "gitee",
        "duckduckgo_images" | "ddg_images" | "ddg_img" | "images" | "image" | "image_search" => "duckduckgo_images",
        "semantic_scholar" | "semanticscholar" | "s2" => "semantic_scholar",
        "dblp" => "dblp",
        "openalex" => "openalex",
        "crossref" | "doi" => "crossref",
        "zhihu" | "zh" => "zhihu",
        "stackoverflow" | "stack-overflow" | "stack_overflow" | "so" | "sof" => "stackoverflow",
        "hackernews" | "hacker-news" | "hacker_news" | "hn" | "ycombinator" => "hackernews",
        "dev_to" | "devto" | "dev.to" => "dev_to",
        "v2ex" => "v2ex",
        "segmentfault" | "segment_fault" | "segfault" | "sf" => "segmentfault",
        "sspai" => "sspai",
        "kr36" | "36kr" | "thirty-six-kr" => "kr36",
        "ithome" => "ithome",
        "reddit" | "r" => "reddit",
        "mastodon" | "fediverse" => "mastodon",
        "weibo" => "weibo",
        "bilibili" | "bili" => "bilibili",
        "invidious" | "youtube" | "yt" => "invidious",
        "wikipedia" | "wiki" | "wikipedia_zh" | "wikipedia_en" => "wikipedia",
        _ => return None,
    })
}

fn default_chain_for(category: SearchCategory) -> &'static [&'static str] {
    match category {
        SearchCategory::Web => &[
            "duckduckgo",
            "bing",
            "jina",
            "brave",
            "searxng",
            "baidu",
            "wikipedia",
            "serper",
            "tavily",
            "exa",
            "csdn",
            "juejin",
            "weixin",
            "zhihu",
            "hackernews",
            "stackoverflow",
            "reddit",
        ],
        SearchCategory::Academic => &[
            "arxiv",
            "openalex",
            "semantic_scholar",
            "crossref",
            "dblp",
            "pubmed",
            "google_scholar",
            "hal",
            "core",
            "biorxiv",
            "ssrn",
            "ieee_xplore",
            "jina",
            "tavily",
            "duckduckgo",
            "bing",
            "baidu",
        ],
        SearchCategory::Code => &[
            "github",
            "github_code_search",
            "github_issues",
            "github_advanced",
            "gitlab",
            "gitee",
            "dblp",
            "stackoverflow",
            "duckduckgo",
            "bing",
            "csdn",
            "juejin",
            "jina",
        ],
        SearchCategory::Cn => &[
            "baidu",
            "csdn",
            "juejin",
            "weixin",
            "zhihu",
            "bing",
            "jina",
            "bilibili",
            "weibo",
            "thepaper",
            "sohu",
            "ithome",
            "kr36",
            "sspai",
            "segmentfault",
            "v2ex",
            "gitee",
            "duckduckgo",
            "searxng",
        ],
        SearchCategory::Social => &[
            "reddit",
            "zhihu",
            "weibo",
            "mastodon",
            "weixin",
            "v2ex",
            "duckduckgo",
            "bing",
            "baidu",
            "csdn",
            "juejin",
        ],
        SearchCategory::News => &[
            "google_news",
            "bing_news",
            "yahoo_news",
            "thepaper",
            "sohu",
            "ithome",
            "kr36",
            "baidu",
            "weixin",
            "bing",
            "jina",
            "tavily",
            "brave",
            "duckduckgo",
            "searxng",
            "csdn",
            "juejin",
            "zhihu",
            "hackernews",
        ],
        SearchCategory::Video => &[
            "bilibili",
            "invidious",
            "bing",
            "duckduckgo",
            "baidu",
            "jina",
        ],
        SearchCategory::Wiki => &[
            "wikipedia",
            "jina",
            "bing",
            "baidu",
            "duckduckgo",
            "searxng",
        ],
        SearchCategory::Lifestyle => &[
            "ithome",
            "sspai",
            "kr36",
            "csdn",
            "juejin",
            "weixin",
            "baidu",
            "bing",
            "duckduckgo",
            "reddit",
        ],
        SearchCategory::Forum => &[
            "stackoverflow",
            "hackernews",
            "dev_to",
            "v2ex",
            "segmentfault",
            "github_issues",
            "reddit",
            "zhihu",
            "csdn",
            "juejin",
            "duckduckgo",
            "bing",
        ],
        SearchCategory::Image => &[
            "duckduckgo_images",
            "bing",
            "baidu",
            "duckduckgo",
        ],
    }
}

fn build_registry() -> EngineRegistry {
    use super::engines as E;
    #[cfg_attr(not(feature = "tool-search-broad"), allow(unused_mut))]
    let mut engines: Vec<Arc<dyn SearchEngine>> = vec![
        Arc::new(E::duckduckgo::DuckDuckGoEngine),
        Arc::new(E::baidu::BaiduEngine),
        Arc::new(E::brave::BraveEngine),
        Arc::new(E::searxng::SearXNGEngine),
        Arc::new(E::github::code::GitHubCodeEngine),
    ];

    #[cfg(feature = "tool-search-broad")]
    {
        engines.push(Arc::new(E::bing::BingEngine));
        engines.push(Arc::new(E::jina::JinaEngine));
        engines.push(Arc::new(E::google::scholar::GoogleScholarEngine));
        engines.push(Arc::new(E::pubmed::PubMedEngine));
        engines.push(Arc::new(E::arxiv::ArxivEngine));
        engines.push(Arc::new(E::csdn::CsdnEngine));
        engines.push(Arc::new(E::juejin::JuejinEngine));
        engines.push(Arc::new(E::weixin::WeixinEngine));
        engines.push(Arc::new(E::serper::SerperEngine));
        engines.push(Arc::new(E::tavily::TavilyEngine));
        engines.push(Arc::new(E::exa::ExaEngine));
        engines.push(Arc::new(E::semantic_scholar::SemanticScholarEngine));
        engines.push(Arc::new(E::openalex::OpenAlexEngine));
        engines.push(Arc::new(E::crossref::CrossrefEngine));
        engines.push(Arc::new(E::dblp::DblpEngine));
        engines.push(Arc::new(E::zhihu::ZhihuEngine));
        engines.push(Arc::new(E::hal::HalEngine));
        engines.push(Arc::new(E::core_engine::CoreEngine));
        engines.push(Arc::new(E::biorxiv::BioRxivEngine));
        engines.push(Arc::new(E::ssrn::SsrnEngine));
        engines.push(Arc::new(E::ieee_xplore::IeeeXploreEngine));
        engines.push(Arc::new(E::google::news::GoogleNewsEngine));
        engines.push(Arc::new(E::bing::news::BingNewsEngine));
        engines.push(Arc::new(E::yahoo_news::YahooNewsEngine));
        engines.push(Arc::new(E::thepaper::ThePaperEngine));
        engines.push(Arc::new(E::sohu::SohuEngine));
        engines.push(Arc::new(E::stackoverflow::StackOverflowEngine));
        engines.push(Arc::new(E::hackernews::HackerNewsEngine));
        engines.push(Arc::new(E::devto::DevToEngine));
        engines.push(Arc::new(E::v2ex::V2exEngine));
        engines.push(Arc::new(E::segmentfault::SegmentFaultEngine));
        engines.push(Arc::new(E::sspai::SspaiEngine));
        engines.push(Arc::new(E::kr36::Kr36Engine));
        engines.push(Arc::new(E::ithome::IthomeEngine));
        engines.push(Arc::new(E::reddit::RedditEngine));
        engines.push(Arc::new(E::mastodon::MastodonEngine));
        engines.push(Arc::new(E::weibo::WeiboEngine));
        engines.push(Arc::new(E::invidious::InvidiousEngine));
        engines.push(Arc::new(E::bilibili::BilibiliEngine));
        engines.push(Arc::new(E::wikipedia::WikipediaEngine));
        engines.push(Arc::new(E::gitlab::GitLabEngine));
        engines.push(Arc::new(E::gitee::GiteeEngine));
        engines.push(Arc::new(E::duckduckgo::images::DuckDuckGoImagesEngine));
        engines.push(Arc::new(E::github::code::search::GitHubCodeSearchEngine));
        engines.push(Arc::new(E::github::issues::GitHubIssuesEngine));
        engines.push(Arc::new(E::github::users::GitHubUsersEngine));
        engines.push(Arc::new(E::github::advanced::GitHubAdvancedEngine));
    }
    EngineRegistry { engines }
}

static GLOBAL_REGISTRY: OnceLock<EngineRegistry> = OnceLock::new();

pub fn global_registry() -> &'static EngineRegistry {
    GLOBAL_REGISTRY.get_or_init(build_registry)
}

pub fn known_engine_ids() -> Vec<&'static str> {
    global_registry().engines.iter().map(|e| e.id()).collect()
}

pub fn known_aliases() -> &'static [&'static str] {
    &[
        "duckduckgo",
        "ddg",
        "brave",
        "searxng",
        "searx",
        "baidu",
        "bing",
        "bing_news",
        "jina",
        "scholar",
        "google_scholar",
        "google_news",
        "yahoo_news",
        "pubmed",
        "arxiv",
        "biorxiv",
        "ssrn",
        "ieee_xplore",
        "hal",
        "core",
        "csdn",
        "juejin",
        "weixin",
        "wechat",
        "sogou",
        "serper",
        "tavily",
        "exa",
        "github",
        "github_code_search",
        "github_issues",
        "github_users",
        "github_advanced",
        "gitlab",
        "gitee",
        "duckduckgo_images",
        "image",
        "image_search",
        "semantic_scholar",
        "s2",
        "dblp",
        "openalex",
        "crossref",
        "doi",
        "zhihu",
        "zh",
        "stackoverflow",
        "hackernews",
        "hn",
        "dev_to",
        "v2ex",
            "segmentfault",
            "sspai",
        "kr36",
            "ithome",
        "reddit",
        "mastodon",
            "weibo",
            "bilibili",
        "invidious",
        "youtube",
        "wikipedia",
        "thepaper",
        "sohu",
    ]
}
