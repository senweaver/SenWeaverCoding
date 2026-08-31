// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::traits::{Tool, ToolResult};
use super::engine::{ApiKeys, SearchCategory, SearchContext, SearchHit, TimeRange};
use super::health;
use super::ranker::{
    academic_merge, filter_by_relevance, merge_and_dedup, render_results_markdown,
    score_and_rank,
};
use super::routing::global_registry;
use super::provider_routing::resolve_web_search_provider;
use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

pub const WEB_SEARCH_DEFAULT_RESULTS: usize = 10;
pub const WEB_SEARCH_MIN_RESULTS_TARGET: usize = 8;
pub const WEB_SEARCH_MAX_FANOUT: usize = 12;
pub const WEB_SEARCH_RESULT_HARD_CAP: usize = 30;
pub const WEB_SEARCH_FIRST_WAVE_SIZE: usize = 4;
pub const WEB_SEARCH_WAVE_INTERVAL_MS: u64 = 2_000;
pub const WEB_SEARCH_SOFT_DEADLINE_SECS: u64 = 8;
pub const WEB_SEARCH_HARD_DEADLINE_MIN_SECS: u64 = 8;
pub const WEB_SEARCH_HARD_DEADLINE_MAX_SECS: u64 = 20;
pub const WEB_SEARCH_PER_ENGINE_TIMEOUT_SECS: u64 = 6;
pub const WEB_SEARCH_PER_ENGINE_RETRY_TIMEOUT_SECS: u64 = 4;
pub const WEB_SEARCH_PER_ENGINE_MIN: usize = 3;

fn normalize_web_search_provider(raw: &str) -> String {
    let resolution = resolve_web_search_provider(raw);
    if resolution.used_fallback && !raw.trim().is_empty() {
        tracing::warn!(
            provider = raw,
            fallback = resolution.canonical_provider,
            route = resolution.route.label(),
            "Unknown web search provider; using default"
        );
    }
    resolution.canonical_provider.to_string()
}

pub struct WebSearchTool {
    provider: String,
    boot_brave_api_key: Option<String>,
    searxng_instance_url: Option<String>,
    boot_tavily_api_key: Option<String>,
    boot_exa_api_key: Option<String>,
    max_results: usize,
    timeout_secs: u64,
    config_path: PathBuf,
    secrets_encrypt: bool,
}

impl WebSearchTool {
    pub fn new(
        provider: String,
        brave_api_key: Option<String>,
        max_results: usize,
        timeout_secs: u64,
    ) -> Self {
        Self {
            provider: normalize_web_search_provider(&provider),
            boot_brave_api_key: brave_api_key,
            searxng_instance_url: None,
            boot_tavily_api_key: None,
            boot_exa_api_key: None,
            max_results: max_results
                .clamp(WEB_SEARCH_DEFAULT_RESULTS, WEB_SEARCH_RESULT_HARD_CAP),
            timeout_secs: timeout_secs.max(1),
            config_path: PathBuf::new(),
            secrets_encrypt: false,
        }
    }

    pub fn new_with_config(
        provider: String,
        brave_api_key: Option<String>,
        searxng_instance_url: Option<String>,
        max_results: usize,
        timeout_secs: u64,
        config_path: PathBuf,
        secrets_encrypt: bool,
    ) -> Self {
        Self {
            provider: normalize_web_search_provider(&provider),
            boot_brave_api_key: brave_api_key,
            searxng_instance_url,
            boot_tavily_api_key: None,
            boot_exa_api_key: None,
            max_results: max_results
                .clamp(WEB_SEARCH_DEFAULT_RESULTS, WEB_SEARCH_RESULT_HARD_CAP),
            timeout_secs: timeout_secs.max(1),
            config_path,
            secrets_encrypt,
        }
    }

    pub fn with_extra_api_keys(
        mut self,
        tavily: Option<String>,
        exa: Option<String>,
    ) -> Self {
        self.boot_tavily_api_key = tavily;
        self.boot_exa_api_key = exa;
        self
    }

    pub fn for_engine(engine_id: impl Into<String>, max_results: usize, timeout_secs: u64) -> Self {
        Self::new(engine_id.into(), None, max_results, timeout_secs)
    }

    fn build_api_keys(&self) -> ApiKeys {
        let brave = decrypt_optional(
            self.boot_brave_api_key.as_deref(),
            &self.config_path,
            self.secrets_encrypt,
        );
        let tavily = decrypt_optional(
            self.boot_tavily_api_key.as_deref(),
            &self.config_path,
            self.secrets_encrypt,
        );
        let exa = decrypt_optional(
            self.boot_exa_api_key.as_deref(),
            &self.config_path,
            self.secrets_encrypt,
        );
        let serper = env_first(&["SEN_SERPER_API_KEY", "SERPER_API_KEY"]);
        let jina = env_first(&["SEN_JINA_API_KEY", "JINA_API_KEY"]);
        let pubmed_email = env_first(&["SEN_PUBMED_EMAIL", "PUBMED_EMAIL"]);
        let github_token = env_first(&["SEN_GITHUB_TOKEN", "GITHUB_TOKEN"]);
        let semantic_scholar = env_first(&["SEN_S2_API_KEY", "SEMANTIC_SCHOLAR_API_KEY"]);
        let core = env_first(&["SEN_CORE_API_KEY", "CORE_API_KEY"]);
        let hal = env_first(&["SEN_HAL_API_KEY", "HAL_API_KEY"]);
        let mastodon_instance = env_first(&[
            "SEN_MASTODON_INSTANCE",
            "MASTODON_INSTANCE",
            "MASTODON_INSTANCE_URL",
        ]);
        let invidious_instance = env_first(&[
            "SEN_INVIDIOUS_INSTANCE",
            "INVIDIOUS_INSTANCE",
            "INVIDIOUS_INSTANCE_URL",
        ]);
        let gitlab_token = env_first(&["SEN_GITLAB_TOKEN", "GITLAB_TOKEN"]);
        let gitee_token = env_first(&["SEN_GITEE_TOKEN", "GITEE_TOKEN", "GITEE_ACCESS_TOKEN"]);
        let youtube_api_key = env_first(&[
            "SEN_YOUTUBE_API_KEY",
            "YOUTUBE_API_KEY",
            "GOOGLE_API_KEY",
        ]);
        ApiKeys {
            brave,
            searxng_url: self
                .searxng_instance_url
                .clone()
                .or_else(|| env_first(&["SEN_SEARXNG_INSTANCE_URL", "SEARXNG_INSTANCE_URL"])),
            tavily,
            exa,
            serper,
            jina,
            pubmed_email,
            github_token,
            semantic_scholar,
            core,
            hal,
            mastodon_instance,
            invidious_instance,
            gitlab_token,
            gitee_token,
            youtube_api_key,
        }
    }

    fn build_context(&self, args: &serde_json::Value) -> anyhow::Result<(SearchContext, bool, Option<String>)> {
        let query = args
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: query"))?
            .trim()
            .to_string();
        if query.is_empty() {
            anyhow::bail!("Search query cannot be empty");
        }
        let requested_limit = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(self.max_results);
        let limit = requested_limit
            .clamp(WEB_SEARCH_DEFAULT_RESULTS, WEB_SEARCH_RESULT_HARD_CAP);
        let explicit_category = args
            .get("category")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty());
        let category = args
            .get("category")
            .and_then(|v| v.as_str())
            .map(SearchCategory::from_str_loose)
            .unwrap_or_else(|| {
                if !explicit_category && query_looks_like_news(&query) {
                    SearchCategory::News
                } else {
                    SearchCategory::Web
                }
            });
        let time_range = args
            .get("time_range")
            .and_then(|v| v.as_str())
            .and_then(TimeRange::from_str_loose);
        let locale = args
            .get("locale")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let safe_search = args
            .get("safe_search")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let multi = args.get("multi").and_then(|v| v.as_bool()).unwrap_or(true);
        let preferred_engine = args
            .get("engine")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());

        let extra = collect_extra_args(args);

        let hard_secs = self.timeout_secs.clamp(
            WEB_SEARCH_HARD_DEADLINE_MIN_SECS,
            WEB_SEARCH_HARD_DEADLINE_MAX_SECS,
        );
        let ctx = SearchContext {
            query,
            limit,
            locale,
            time_range,
            safe_search,
            category,
            timeout: Duration::from_secs(hard_secs),
            api_keys: self.build_api_keys(),
            user_agent: super::engine::default_user_agent(),
            extra,
        };
        Ok((ctx, multi, preferred_engine))
    }
}

const EXTRA_PASSTHROUGH_KEYS: &[&str] = &[
    "search_depth",
    "include_answer",
    "include_raw_content",
    "include_domains",
    "exclude_domains",
    "exa_type",
    "type",
    "get_contents",
    "highlight_sentences",
    "category_filter",
    "subreddit",
    "sort",
    "time_filter",
    "t",
    "sort_by",
    "size",
    "image_type",
    "search_type",
    "order",
    "owners",
    "repos",
    "languages",
    "topics",
    "labels",
    "in_fields",
    "license",
    "stars",
    "forks",
    "size_kb",
    "followers",
    "created",
    "pushed",
    "updated",
    "merged",
    "closed",
    "good_first_issues",
    "help_wanted_issues",
    "filename",
    "extension",
    "path",
    "state",
    "milestone",
    "linked",
    "review",
    "reviewed_by",
    "review_requested",
    "team_review_requested",
    "author",
    "assignee",
    "mentions",
    "team",
    "commenter",
    "involves",
    "comments",
    "interactions",
    "reactions",
    "head",
    "base",
    "status",
    "language_in_user",
    "location",
    "archived",
    "is_mirror",
    "is_template",
    "is_draft",
    "is_public",
    "is_private",
    "is_fork",
    "no_label",
    "no_milestone",
    "no_assignee",
    "draft",
];

fn collect_extra_args(args: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    let mut extra = serde_json::Map::new();
    let Some(obj) = args.as_object() else {
        return extra;
    };
    for key in EXTRA_PASSTHROUGH_KEYS {
        if let Some(v) = obj.get(*key) {
            if !v.is_null() {
                extra.insert((*key).to_string(), v.clone());
            }
        }
    }
    if let Some(custom) = obj.get("extra").and_then(|v| v.as_object()) {
        for (k, v) in custom.iter() {
            if !v.is_null() {
                extra.insert(k.clone(), v.clone());
            }
        }
    }
    extra
}

fn query_looks_like_news(query: &str) -> bool {
    let lower = query.to_lowercase();
    const ZH_NEWS: &[&str] = &[
        "新闻", "热点", "热搜", "今日", "今天", "最新", "快讯",
        "早报", "晚报", "头条", "实时", "速报", "聚焦", "突发",
    ];
    const EN_NEWS: &[&str] = &[
        "news", "latest", "breaking", "today", "headline", "headlines",
        "live updates", "this week", "trending",
    ];
    if ZH_NEWS.iter().any(|kw| query.contains(kw)) {
        return true;
    }
    if EN_NEWS.iter().any(|kw| lower.contains(kw)) {
        return true;
    }
    false
}

fn is_captcha_block_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    if lower.contains("captcha")
        || lower.contains("robot check")
        || lower.contains("are you human")
        || lower.contains("verify you are not a robot")
    {
        return true;
    }
    message.contains("请输入验证码")
        || message.contains("百度安全验证")
        || message.contains("安全验证")
        || message.contains("人机验证")
}

fn is_transient_network_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "error sending request",
        "connection reset",
        "connection closed",
        "timed out",
        "timeout",
        "temporary failure",
        "broken pipe",
        "tls handshake",
        "dns",
        "503",
        "504",
        "502",
    ]
    .iter()
    .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
}

fn compute_per_engine_limit(global_limit: usize, fanout: usize) -> usize {
    if fanout == 0 {
        return global_limit;
    }
    let base = global_limit.div_ceil(fanout);
    base.saturating_add(2).max(WEB_SEARCH_PER_ENGINE_MIN)
}

async fn run_engine_with_retry(
    engine: &Arc<dyn super::engine::SearchEngine>,
    ctx: &SearchContext,
    hard_deadline: tokio::time::Instant,
) -> anyhow::Result<Vec<SearchHit>> {
    let remaining = hard_deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining < Duration::from_millis(200) {
        anyhow::bail!("engine skipped: hard deadline reached");
    }
    let first_budget = Duration::from_secs(WEB_SEARCH_PER_ENGINE_TIMEOUT_SECS).min(remaining);
    let first = match tokio::time::timeout(first_budget, engine.search(ctx)).await {
        Ok(res) => res,
        Err(_) => Err(anyhow::anyhow!(
            "engine timeout after {:.1}s",
            first_budget.as_secs_f32()
        )),
    };
    let err = match first {
        Ok(hits) => return Ok(hits),
        Err(err) => err,
    };
    let msg = err.to_string();
    if is_captcha_block_error(&msg) {
        tracing::info!(
            target: "tools.web_search",
            engine = engine.id(),
            "engine triggered captcha/anti-bot; skipping retry"
        );
        return Err(err);
    }
    let transient = is_transient_network_error(&msg) || msg.contains("engine timeout after");
    if !transient {
        return Err(err);
    }
    let remaining = hard_deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining < Duration::from_millis(700) {
        return Err(err);
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    tracing::debug!(
        target: "tools.web_search",
        engine = engine.id(),
        error = %msg,
        "engine returned transient network error; retrying once"
    );
    let retry_budget = Duration::from_secs(WEB_SEARCH_PER_ENGINE_RETRY_TIMEOUT_SECS)
        .min(hard_deadline.saturating_duration_since(tokio::time::Instant::now()));
    match tokio::time::timeout(retry_budget, engine.search(ctx)).await {
        Ok(res) => res,
        Err(_) => Err(anyhow::anyhow!(
            "engine timeout after {:.1}s (retry)",
            retry_budget.as_secs_f32()
        )),
    }
}

fn env_first(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(v) = std::env::var(key) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn decrypt_optional(value: Option<&str>, config_path: &Path, secrets_encrypt: bool) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    if !crate::security::SecretStore::is_encrypted(raw) {
        return Some(raw.to_string());
    }
    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let store = crate::security::SecretStore::new(parent, secrets_encrypt);
    store.decrypt(raw).ok().filter(|s| !s.is_empty())
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search_tool"
    }

    fn description(&self) -> &str {
        "Search the web across many engines in parallel with staggered waves and early \
         termination (DuckDuckGo/Bing/Brave/SearXNG/Baidu/Jina/Tavily/Exa/Serper/Google \
         Scholar/PubMed/arXiv/Semantic Scholar/OpenAlex/Crossref/DBLP/CSDN/Juejin/Weixin/\
         Zhihu/GitHub). Supports category (web/academic/code/cn/news/social), time_range, \
         locale. The first wave queries the 4 healthiest engines; additional waves launch \
         every 2s only while results are still insufficient (up to 12 engines total). The \
         tool returns as soon as enough deduplicated results arrive from at least 2 engines \
         (typically 1-4s), at the 8s soft deadline when any results exist, and always by \
         the hard deadline (8-20s, from web_search.timeout_secs). Engines that recently \
         timed out or served a captcha are put in cooldown and skipped on later queries, so \
         blocked engines (e.g. DuckDuckGo on China-hosted networks) never slow future \
         searches; CJK queries auto-promote Baidu/Bing/Jina/CSDN/Juejin/Weixin/Zhihu. \
         Results are merged, deduplicated by URL, and ranked by keyword coverage, engine \
         authority, cross-engine corroboration, per-engine rank and freshness; for \
         category=academic the same paper from arXiv/OpenAlex/Semantic Scholar is collapsed \
         into a single richer entry via DOI/arXiv id/normalized title. Engine failures \
         (captcha/transient network/timeout) are tolerated silently; if every engine \
         returns zero the tool still returns success=true with a soft-empty diagnostic \
         (NOT a hard error), so re-issue a reworded query instead of treating it as a \
         fatal tool failure."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query." },
                "engine": {
                    "type": "string",
                    "description": "Optional preferred engine id (e.g. duckduckgo, bing, brave, searxng, baidu, jina, tavily, exa, serper, google_scholar, pubmed, arxiv, semantic_scholar, openalex, crossref, dblp, csdn, juejin, weixin, zhihu, github). A preferred engine is queried even if it is in health cooldown."
                },
                "category": {
                    "type": "string",
                    "enum": ["web", "academic", "code", "cn", "news", "social"],
                    "description": "Search category. Steers default engine selection and fallback chain."
                },
                "time_range": {
                    "type": "string",
                    "enum": ["day", "week", "month", "year"],
                    "description": "Optional freshness filter (honoured by engines that support it)."
                },
                "locale": { "type": "string", "description": "Optional locale hint, e.g. en, zh-CN, ja." },
                "max_results": { "type": "integer", "minimum": 1, "maximum": 30, "description": "Max results to return (default 10; merged across engines)." },
                "multi": { "type": "boolean", "description": "Legacy hint; the tool always uses staggered multi-engine waves with early termination regardless of this value. Keep default true." },
                "safe_search": { "type": "boolean", "description": "Enable safe search filtering (default true)." }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let (merged, diag) = self.search_hits_internal(&args).await?;
        let outcome = classify_outcome(&merged, &diag);
        let mut output = String::new();
        match outcome {
            SearchOutcome::Ok => {
                if !diag.successful_engines.is_empty() {
                    output.push_str(&format!(
                        "Sources: {} engine(s) returned {} aggregated result(s) in {}ms — {}\n\n",
                        diag.successful_engines.len(),
                        merged.len(),
                        diag.elapsed_ms,
                        diag.successful_engines.join(", ")
                    ));
                }
                output.push_str(&render_results_markdown(&diag.query, &merged));
            }
            SearchOutcome::Empty => {
                output.push_str(&render_results_markdown(&diag.query, &merged));
                output.push_str(
                    "\n\nNo usable results returned for this query, but the engines themselves \
                     responded normally. If you still need search results, rewrite the query \
                     (different keywords, drop date constraints, or simplify). Calling web_search \
                     again with the same query will be deduplicated and will not run. \
                     This is NOT a hard tool failure.",
                );
            }
            SearchOutcome::NetworkError => {
                output.push_str(&format!(
                    "Web search is temporarily unavailable: all {} engine attempt(s) failed with \
                     network/timeout errors within {}ms. This is a CONNECTIVITY problem, not a \
                     query problem — do NOT rewrite the query and do NOT retry immediately. \
                     Inform the user that web search is unreachable (likely network, proxy or \
                     firewall) and continue with local knowledge where possible.\n\
                     网络连接异常:{} 个搜索引擎均无法访问,请检查网络连接或代理设置;这不是查询词的问题。",
                    diag.tried_engines.max(diag.errors.len()),
                    diag.elapsed_ms,
                    diag.tried_engines.max(diag.errors.len()),
                ));
                append_failed_engine_lines(&mut output, &diag);
            }
            SearchOutcome::Blocked => {
                output.push_str(&format!(
                    "Search engines blocked automated access for this query (captcha/anti-bot): {}. \
                     The affected engines are now in cooldown, so retrying immediately will not \
                     help. Suggest configuring API-key based engines (Tavily/Serper/Exa/Brave) for \
                     reliable access, or retry later.\n\
                     搜索引擎触发了人机验证,相关引擎已进入冷却;请稍后重试,或配置 API 引擎(Tavily/Serper/Exa/Brave)以获得稳定搜索。",
                    diag.captcha_blocked.join(", "),
                ));
                append_failed_engine_lines(&mut output, &diag);
            }
        }
        if let Some(envelope) = render_web_search_envelope(&diag, &merged, outcome) {
            output.push_str("\n\n");
            output.push_str(&envelope);
        }
        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOutcome {
    Ok,
    Empty,
    NetworkError,
    Blocked,
}

impl SearchOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Empty => "empty",
            Self::NetworkError => "network_error",
            Self::Blocked => "blocked",
        }
    }
}

fn classify_outcome(merged: &[SearchHit], diag: &SearchDiagnostics) -> SearchOutcome {
    if !merged.is_empty() {
        return SearchOutcome::Ok;
    }
    let responded = diag.successful_engines.len() + diag.empty_engines.len();
    if responded == 0 && !diag.errors.is_empty() {
        if diag.captcha_blocked.len() * 2 >= diag.errors.len() {
            SearchOutcome::Blocked
        } else {
            SearchOutcome::NetworkError
        }
    } else {
        SearchOutcome::Empty
    }
}

fn truncate_reason(msg: &str) -> String {
    const MAX_CHARS: usize = 120;
    if msg.chars().count() <= MAX_CHARS {
        return msg.to_string();
    }
    let mut out: String = msg.chars().take(MAX_CHARS).collect();
    out.push('…');
    out
}

fn failed_engine_entries(diag: &SearchDiagnostics) -> Vec<(String, String)> {
    diag.errors
        .iter()
        .filter(|entry| !entry.starts_with('('))
        .take(6)
        .map(|entry| match entry.split_once(": ") {
            Some((engine, reason)) => (engine.to_string(), truncate_reason(reason)),
            None => ("search".to_string(), truncate_reason(entry)),
        })
        .collect()
}

fn append_failed_engine_lines(output: &mut String, diag: &SearchDiagnostics) {
    let failed = failed_engine_entries(diag);
    if failed.is_empty() {
        return;
    }
    output.push_str("\n\nFailed engines:");
    for (engine, reason) in failed {
        output.push_str(&format!("\n- {engine}: {reason}"));
    }
}

fn outcome_notice(outcome: SearchOutcome, diag: &SearchDiagnostics) -> Option<String> {
    match outcome {
        SearchOutcome::Ok => None,
        SearchOutcome::Empty => Some(
            "各引擎均无匹配结果,请尝试更换关键词。No engine returned matching results — try different keywords."
                .to_string(),
        ),
        SearchOutcome::NetworkError => Some(format!(
            "网络连接异常:{} 个搜索引擎均无法访问,请检查网络或代理设置。Web search unreachable — none of the engines responded; check network/proxy.",
            diag.tried_engines.max(diag.errors.len()),
        )),
        SearchOutcome::Blocked => Some(
            "搜索引擎触发人机验证,已进入冷却;请稍后重试或配置 API 引擎(Tavily/Serper/Exa)。Engines served captcha challenges and are cooling down; retry later or configure API-based engines."
                .to_string(),
        ),
    }
}

fn host_from_url(url: &str) -> String {
    let lower = url.trim();
    let stripped = lower
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("ftp://")
        .trim_start_matches("//");
    let host = stripped
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(stripped)
        .trim_start_matches("www.");
    let host = host.split('@').next_back().unwrap_or(host);
    host.split(':').next().unwrap_or(host).to_string()
}

fn render_web_search_envelope(
    diag: &SearchDiagnostics,
    merged: &[SearchHit],
    outcome: SearchOutcome,
) -> Option<String> {
    let hits_json: Vec<serde_json::Value> = merged
        .iter()
        .enumerate()
        .map(|(idx, h)| {
            serde_json::json!({
                "index": idx + 1,
                "title": h.title,
                "url": h.url,
                "host": host_from_url(&h.url),
                "description": h.description,
                "source": h.source.clone().unwrap_or_default(),
                "engine": h.engine,
                "publishedAt": h.published_at.clone().unwrap_or_default(),
            })
        })
        .collect();
    let failed_json: Vec<serde_json::Value> = failed_engine_entries(diag)
        .into_iter()
        .map(|(engine, reason)| serde_json::json!({ "engine": engine, "reason": reason }))
        .collect();
    let payload = serde_json::json!({
        "query": diag.query,
        "status": outcome.label(),
        "notice": outcome_notice(outcome, diag),
        "successful_engines": diag.successful_engines,
        "failed_engines": failed_json,
        "captcha_blocked": diag.captcha_blocked,
        "tried_engines": diag.tried_engines,
        "elapsed_ms": diag.elapsed_ms,
        "hits": hits_json,
    });
    let json = serde_json::to_string(&payload).ok()?;
    Some(format!(
        "===WEB_SEARCH_JSON_BEGIN===\n{json}\n===WEB_SEARCH_JSON_END==="
    ))
}

pub struct SearchDiagnostics {
    pub query: String,
    pub primary_engines: Vec<String>,
    pub successful_engines: Vec<String>,
    pub empty_engines: Vec<String>,
    pub errors: Vec<String>,
    pub captcha_blocked: Vec<String>,
    pub elapsed_ms: u64,
    pub tried_engines: usize,
}

type EngineTaskOutput = (String, String, anyhow::Result<Vec<SearchHit>>);

fn spawn_engine_wave(
    join: &mut JoinSet<EngineTaskOutput>,
    engines: &[Arc<dyn super::engine::SearchEngine>],
    ctx: &SearchContext,
    per_engine_limit: usize,
    hard_deadline: tokio::time::Instant,
) {
    for engine in engines {
        let engine = engine.clone();
        let mut ctx_for_engine = ctx.clone();
        ctx_for_engine.limit = per_engine_limit;
        join.spawn(async move {
            let attempt_started = tokio::time::Instant::now();
            let res = run_engine_with_retry(&engine, &ctx_for_engine, hard_deadline).await;
            match &res {
                Ok(_) => health::record_success(engine.id(), attempt_started.elapsed()),
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("engine skipped: hard deadline") {
                        health::record_failure(engine.id(), health::classify_failure(&msg));
                    }
                }
            }
            (engine.label().to_string(), engine.id().to_string(), res)
        });
    }
}

impl WebSearchTool {
    pub async fn search_hits(
        &self,
        args: serde_json::Value,
    ) -> anyhow::Result<Vec<SearchHit>> {
        let (merged, _diag) = self.search_hits_internal(&args).await?;
        Ok(merged)
    }

    async fn search_hits_internal(
        &self,
        args: &serde_json::Value,
    ) -> anyhow::Result<(Vec<SearchHit>, SearchDiagnostics)> {
        let (ctx, multi, preferred_raw) = self.build_context(args)?;
        let registry = global_registry();
        let preferred = preferred_raw.as_deref().or_else(|| {
            if self.provider.is_empty() || self.provider == "default" {
                None
            } else {
                Some(self.provider.as_str())
            }
        });
        let engine_only = args
            .get("engine_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let chain = if engine_only {
            if let Some(p) = preferred.and_then(|p| registry.find(p)) {
                vec![p]
            } else {
                anyhow::bail!(
                    "engine_only=true requires a recognised engine id (got {:?})",
                    preferred
                );
            }
        } else {
            registry.fallback_chain(ctx.category, &ctx.api_keys, preferred, &ctx.query)
        };
        if chain.is_empty() {
            anyhow::bail!(
                "No search engine available for query '{}' (category={}).",
                ctx.query,
                ctx.category.label()
            );
        }

        tracing::info!(
            "web_search query='{}' category={} multi={} chain=[{}]",
            ctx.query,
            ctx.category.label(),
            multi,
            chain
                .iter()
                .map(|e| e.id())
                .collect::<Vec<_>>()
                .join(",")
        );

        let started = tokio::time::Instant::now();
        let hard_secs = self.timeout_secs.clamp(
            WEB_SEARCH_HARD_DEADLINE_MIN_SECS,
            WEB_SEARCH_HARD_DEADLINE_MAX_SECS,
        );
        let hard_deadline = started + Duration::from_secs(hard_secs);
        let soft_deadline =
            started + Duration::from_secs(WEB_SEARCH_SOFT_DEADLINE_SECS.min(hard_secs));

        let capped: Vec<Arc<dyn super::engine::SearchEngine>> =
            chain.iter().take(WEB_SEARCH_MAX_FANOUT).cloned().collect();
        let total_engines = capped.len();
        let mut waves: Vec<Vec<Arc<dyn super::engine::SearchEngine>>> = Vec::new();
        {
            let mut rest = capped;
            while !rest.is_empty() {
                let take = WEB_SEARCH_FIRST_WAVE_SIZE.min(rest.len());
                waves.push(rest.drain(..take).collect());
            }
        }
        let primary_labels: Vec<String> = waves
            .first()
            .map(|wave| wave.iter().map(|e| e.label().to_string()).collect())
            .unwrap_or_default();
        let later_wave_limit =
            compute_per_engine_limit(ctx.limit, WEB_SEARCH_FIRST_WAVE_SIZE);

        let quorum_engines = 2usize.min(total_engines.max(1));
        let target_hits = ctx.limit.max(WEB_SEARCH_MIN_RESULTS_TARGET);

        let mut join: JoinSet<EngineTaskOutput> = JoinSet::new();
        spawn_engine_wave(&mut join, &waves[0], &ctx, ctx.limit, hard_deadline);
        let mut engines_launched = waves[0].len();
        let mut next_wave_idx = 1usize;
        let mut next_wave_at =
            started + Duration::from_millis(WEB_SEARCH_WAVE_INTERVAL_MS);

        let mut streams: Vec<Vec<SearchHit>> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut empty_engines: Vec<String> = Vec::new();
        let mut captcha_blocked: Vec<String> = Vec::new();
        let mut successful_engines: Vec<String> = Vec::new();
        let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut unique_hits = 0usize;
        let mut soft_fired = false;

        loop {
            tokio::select! {
                biased;
                joined = join.join_next() => {
                    match joined {
                        Some(Ok((label, _id, result))) => {
                            match result {
                                Ok(hits) if !hits.is_empty() => {
                                    for hit in &hits {
                                        if seen_keys.insert(hit.dedup_key()) {
                                            unique_hits += 1;
                                        }
                                    }
                                    successful_engines.push(label);
                                    streams.push(hits);
                                    if successful_engines.len() >= quorum_engines
                                        && unique_hits >= target_hits
                                    {
                                        tracing::debug!(
                                            target: "tools.web_search",
                                            unique_hits,
                                            engines = successful_engines.len(),
                                            elapsed_ms = started.elapsed().as_millis() as u64,
                                            "early termination: result quorum reached"
                                        );
                                        break;
                                    }
                                }
                                Ok(_) => empty_engines.push(label),
                                Err(e) => {
                                    let msg = e.to_string();
                                    if is_captcha_block_error(&msg) {
                                        captcha_blocked.push(label.clone());
                                        tracing::warn!(
                                            target: "tools.web_search",
                                            engine = %label,
                                            "engine returned captcha challenge; skipping in this run"
                                        );
                                    } else {
                                        tracing::warn!(
                                            target: "tools.web_search",
                                            engine = %label,
                                            error = %msg,
                                            "engine failed; degrading to zero results from this source"
                                        );
                                    }
                                    errors.push(format!("{label}: {msg}"));
                                }
                            }
                        }
                        Some(Err(join_err)) => {
                            if !join_err.is_cancelled() {
                                errors.push(format!("(join) {join_err}"));
                            }
                        }
                        None => {
                            if next_wave_idx < waves.len() {
                                spawn_engine_wave(
                                    &mut join,
                                    &waves[next_wave_idx],
                                    &ctx,
                                    later_wave_limit,
                                    hard_deadline,
                                );
                                engines_launched += waves[next_wave_idx].len();
                                next_wave_idx += 1;
                                next_wave_at = tokio::time::Instant::now()
                                    + Duration::from_millis(WEB_SEARCH_WAVE_INTERVAL_MS);
                            } else {
                                break;
                            }
                        }
                    }
                }
                _ = tokio::time::sleep_until(next_wave_at), if next_wave_idx < waves.len() => {
                    if unique_hits < target_hits
                        || successful_engines.len() < quorum_engines
                    {
                        spawn_engine_wave(
                            &mut join,
                            &waves[next_wave_idx],
                            &ctx,
                            later_wave_limit,
                            hard_deadline,
                        );
                        engines_launched += waves[next_wave_idx].len();
                        next_wave_idx += 1;
                    }
                    next_wave_at = tokio::time::Instant::now()
                        + Duration::from_millis(WEB_SEARCH_WAVE_INTERVAL_MS);
                }
                _ = tokio::time::sleep_until(soft_deadline), if !soft_fired => {
                    soft_fired = true;
                    if unique_hits > 0 {
                        tracing::debug!(
                            target: "tools.web_search",
                            unique_hits,
                            "soft deadline reached with partial results; returning early"
                        );
                        break;
                    }
                    while next_wave_idx < waves.len() {
                        spawn_engine_wave(
                            &mut join,
                            &waves[next_wave_idx],
                            &ctx,
                            later_wave_limit,
                            hard_deadline,
                        );
                        engines_launched += waves[next_wave_idx].len();
                        next_wave_idx += 1;
                    }
                }
                _ = tokio::time::sleep_until(hard_deadline) => {
                    tracing::warn!(
                        target: "tools.web_search",
                        "hard deadline after {}s; returning partial results",
                        hard_secs
                    );
                    errors.push(format!(
                        "(hard deadline after {hard_secs}s; partial results)"
                    ));
                    break;
                }
            }
        }
        join.abort_all();

        let interleaved = merge_and_dedup(streams, ctx.limit.saturating_mul(2));
        let consolidated = if matches!(ctx.category, SearchCategory::Academic) {
            academic_merge(interleaved)
        } else {
            interleaved
        };
        let filtered = filter_by_relevance(consolidated, &ctx.query);
        let ranked = score_and_rank(
            filtered,
            &ctx.query,
            ctx.category,
            ctx.time_range.is_some(),
        );
        let merged: Vec<SearchHit> = ranked.into_iter().take(ctx.limit).collect();
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if merged.is_empty() {
            tracing::warn!(
                target: "tools.web_search",
                query = %ctx.query,
                tried = chain.len(),
                successful = ?successful_engines,
                elapsed_ms,
                "all engines returned zero usable results; surfacing soft-empty payload"
            );
        } else {
            tracing::info!(
                target: "tools.web_search",
                query = %ctx.query,
                hits = merged.len(),
                successful = ?successful_engines,
                elapsed_ms,
                "web search ok"
            );
        }

        let diag = SearchDiagnostics {
            query: ctx.query.clone(),
            primary_engines: primary_labels,
            successful_engines,
            empty_engines,
            errors,
            captcha_blocked,
            elapsed_ms,
            tried_engines: engines_launched,
        };
        Ok((merged, diag))
    }
}
