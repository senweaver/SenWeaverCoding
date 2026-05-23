// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static LAST_WEB_SEARCH_FAILURE_MS: AtomicU64 = AtomicU64::new(0);

const FAILURE_GRACE_WINDOW_MS: u64 = 10 * 60 * 1000;

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub fn record_web_search_failure() {
    LAST_WEB_SEARCH_FAILURE_MS.store(now_unix_ms(), Ordering::Relaxed);
}

pub fn record_web_search_success() {
    LAST_WEB_SEARCH_FAILURE_MS.store(0, Ordering::Relaxed);
}

pub fn web_search_recently_failed() -> bool {
    let last = LAST_WEB_SEARCH_FAILURE_MS.load(Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    now_unix_ms().saturating_sub(last) <= FAILURE_GRACE_WINDOW_MS
}

pub fn is_web_search_tool_name(name: &str) -> bool {
    matches!(
        name,
        "web_search" | "web_search_tool" | "tavily_search" | "exa_search" | "multi_search"
    )
}

const SEARCH_ENGINE_DOMAINS: &[&str] = &[
    "baidu.com",
    "google.com",
    "google.co.jp",
    "google.co.uk",
    "google.de",
    "google.fr",
    "google.com.hk",
    "google.com.tw",
    "google.com.sg",
    "google.com.au",
    "google.ca",
    "bing.com",
    "duckduckgo.com",
    "yandex.com",
    "yandex.ru",
    "yahoo.com",
    "search.yahoo.com",
    "sogou.com",
    "so.com",
    "search.brave.com",
    "ecosia.org",
    "startpage.com",
    "qwant.com",
    "kagi.com",
];

struct AggregatorRule {
    host_suffix: &'static str,
    path_prefix: Option<&'static str>,
    category_hint: &'static str,
}

const NEWS_AGGREGATOR_RULES: &[AggregatorRule] = &[
    AggregatorRule { host_suffix: "top.baidu.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "tophub.today", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "s.weibo.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "d.weibo.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "hot.weibo.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "trends.google.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "news.google.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "news.baidu.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "news.bing.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "news.sina.com.cn", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "news.qq.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "news.163.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "news.sohu.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "toutiao.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "36kr.com", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "thepaper.cn", path_prefix: None, category_hint: "news" },
    AggregatorRule { host_suffix: "zhihu.com", path_prefix: Some("/hot"), category_hint: "news" },
    AggregatorRule { host_suffix: "zhihu.com", path_prefix: Some("/billboard"), category_hint: "news" },
    AggregatorRule { host_suffix: "v2ex.com", path_prefix: Some("/hot"), category_hint: "forum" },
    AggregatorRule { host_suffix: "hacker-news.firebaseio.com", path_prefix: None, category_hint: "forum" },
    AggregatorRule { host_suffix: "news.ycombinator.com", path_prefix: None, category_hint: "forum" },
];

fn host_matches_aggregator(host: &str, rule: &AggregatorRule) -> bool {
    host == rule.host_suffix || host.ends_with(&format!(".{}", rule.host_suffix))
}

fn aggregator_match(host: &str, path_only: &str) -> Option<&'static AggregatorRule> {
    let host_norm = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let host_norm = host_norm
        .strip_prefix("www.")
        .or_else(|| host_norm.strip_prefix("m."))
        .unwrap_or(host_norm.as_str())
        .to_string();
    for rule in NEWS_AGGREGATOR_RULES.iter() {
        if !host_matches_aggregator(&host_norm, rule) {
            continue;
        }
        if let Some(prefix) = rule.path_prefix {
            if !path_only.starts_with(prefix) {
                continue;
            }
        }
        return Some(rule);
    }
    None
}

const SEARCH_QUERY_KEYS: &[&str] = &[
    "q",
    "wd",
    "query",
    "p",
    "kw",
    "text",
    "search_query",
    "keyword",
    "k",
];

pub fn is_search_engine_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    let stripped = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .or_else(|| host.strip_prefix("html."))
        .or_else(|| host.strip_prefix("search."))
        .or_else(|| host.strip_prefix("cn."))
        .unwrap_or(host.as_str());
    SEARCH_ENGINE_DOMAINS
        .iter()
        .any(|d| stripped == *d || host == *d || host.ends_with(&format!(".{}", d)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MisuseKind {
    SearchEngine,
    Aggregator,
}

#[derive(Debug, Clone)]
pub struct SearchEngineMisuse {
    pub original_url: String,
    pub host: String,
    pub query: String,
    pub kind: MisuseKind,
    pub suggested_category: &'static str,
}

fn parse_host(url: &str) -> Option<(String, String)> {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let (host_and_port, path_and_query) = match after_scheme.find('/') {
        Some(i) => after_scheme.split_at(i),
        None => (after_scheme, ""),
    };
    let host = host_and_port
        .split('@')
        .next_back()
        .unwrap_or(host_and_port)
        .split(':')
        .next()
        .unwrap_or(host_and_port)
        .to_string();
    if host.is_empty() {
        return None;
    }
    Some((host, path_and_query.to_string()))
}

fn extract_query_from_path_and_query(path_and_query: &str) -> Option<String> {
    let q_idx = path_and_query.find('?')?;
    let qs = &path_and_query[q_idx + 1..];
    let qs = qs.split('#').next().unwrap_or(qs);
    for pair in qs.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        if !SEARCH_QUERY_KEYS.iter().any(|k| key.eq_ignore_ascii_case(k)) {
            continue;
        }
        let value_normalized = value.replace('+', " ");
        let decoded = match urlencoding::decode(&value_normalized) {
            Ok(s) => s.into_owned(),
            Err(_) => value_normalized,
        };
        let trimmed = decoded.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

pub fn classify_search_engine_url(raw: &str) -> Option<SearchEngineMisuse> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (host, path_and_query) = parse_host(raw)?;
    let path_only = path_only_from_path_and_query(&path_and_query);
    if let Some(rule) = aggregator_match(&host, &path_only) {
        let synthetic_query = extract_query_from_path_and_query(&path_and_query)
            .unwrap_or_else(|| host.clone());
        return Some(SearchEngineMisuse {
            original_url: raw.to_string(),
            host,
            query: synthetic_query,
            kind: MisuseKind::Aggregator,
            suggested_category: rule.category_hint,
        });
    }
    if is_search_engine_host(&host) {
        let query = extract_query_from_path_and_query(&path_and_query)?;
        return Some(SearchEngineMisuse {
            original_url: raw.to_string(),
            host,
            query,
            kind: MisuseKind::SearchEngine,
            suggested_category: "web",
        });
    }
    None
}

fn path_only_from_path_and_query(path_and_query: &str) -> String {
    let s = path_and_query.split('?').next().unwrap_or(path_and_query);
    let s = s.split('#').next().unwrap_or(s);
    s.to_string()
}

pub fn detect_misuse_for_browser(args: &serde_json::Value) -> Option<SearchEngineMisuse> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    if !matches!(
        action,
        "open" | "open_tab" | "navigate" | "goto" | "load" | ""
    ) {
        return None;
    }
    let url = args.get("url").and_then(|v| v.as_str())?;
    classify_search_engine_url(url)
}

pub fn detect_misuse_for_web_fetch(args: &serde_json::Value) -> Option<SearchEngineMisuse> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("target").and_then(|v| v.as_str()))?;
    classify_search_engine_url(url)
}

pub fn refusal_message(
    tool_name: &str,
    misuse: &SearchEngineMisuse,
    web_search_enabled: bool,
) -> String {
    let directive = if web_search_enabled {
        match misuse.kind {
            MisuseKind::SearchEngine => format!(
                "First call `web_search` with the same intent (e.g. \
                 web_search(query=\"{}\")). web_search has built-in multi-provider failover \
                 (DuckDuckGo \u{2192} Baidu \u{2192} Bing \u{2192} SearXNG \u{2192} Brave \u{2192} \
                 Jina \u{2192} Tavily etc.) and returns structured results. ONLY if web_search \
                 comes back with a real failure (\"All web search providers failed: ...\") may you \
                 fall back to opening a search-engine results page in `browser` / `web_fetch` \
                 \u{2014} the runtime will then automatically allow that for the next 10 minutes.",
                misuse.query.replace('"', "'")
            ),
            MisuseKind::Aggregator => format!(
                "This URL is a hot-list / trending aggregator page; fetching it directly returns \
                 raw HTML that is rarely useful. INSTEAD call `web_search` with the appropriate \
                 category, e.g. web_search(query=\"{q}\", category=\"{cat}\") \u{2014} the registry \
                 already includes Google News, Bing News, Sohu News, ThePaper, etc. on the news \
                 fallback chain. Use multi-engine fan-out to get structured per-article results \
                 you can then web_fetch ONE BY ONE if you need full text. If web_search fails (\
                 \"All providers failed\") the runtime will allow this fetch automatically for the \
                 next 10 minutes.",
                q = misuse.query.replace('"', "'"),
                cat = misuse.suggested_category
            ),
        }
    } else {
        "Web research is currently disabled in Settings \u{2192} Tools & MCPs \u{2192} \
         Web Research. Tell the user that web research is off; do NOT use the embedded \
         browser to fetch a search-engine results page as a workaround."
            .to_string()
    };
    let kind_label = match misuse.kind {
        MisuseKind::SearchEngine => "search-engine results URL",
        MisuseKind::Aggregator => "hot-list / news aggregator URL",
    };
    format!(
        "[Refused] `{tool_name}` was called with a {kind_label} ({}, query: \"{}\") \
         but `web_search` has not been tried yet in this session.\n\
         Note: the embedded browser usually renders such pages as blank \
         (X-Frame-Options / cross-origin protections), so this fallback is only useful if \
         `web_search` itself is unreachable. {directive}",
        misuse.host, misuse.query
    )
}

pub enum GuardDecision {
    Allow,
    AllowWithFallbackTrace,
    Refuse(String),
}

pub fn evaluate_browser_or_web_fetch_call(
    tool_name: &str,
    args: &serde_json::Value,
    web_search_config_enabled: bool,
) -> GuardDecision {
    let misuse = match tool_name {
        "browser" => detect_misuse_for_browser(args),
        "web_fetch" => detect_misuse_for_web_fetch(args),
        _ => None,
    };
    let Some(misuse) = misuse else {
        return GuardDecision::Allow;
    };
    if web_search_recently_failed() {
        return GuardDecision::AllowWithFallbackTrace;
    }
    GuardDecision::Refuse(refusal_message(
        tool_name,
        &misuse,
        web_search_config_enabled,
    ))
}
