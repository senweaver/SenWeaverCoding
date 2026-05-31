// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::traits::{Tool, ToolResult};
use super::engine::{ApiKeys, SearchCategory, SearchContext, SearchHit, TimeRange};
use super::ranker::{
    academic_merge, filter_by_relevance, merge_and_dedup, render_results_markdown,
};
use super::routing::global_registry;
use super::provider_routing::resolve_web_search_provider;
use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const WEB_SEARCH_DEFAULT_RESULTS: usize = 10;
pub const WEB_SEARCH_MIN_RESULTS_TARGET: usize = 8;
pub const WEB_SEARCH_MAX_FANOUT: usize = 12;
pub const WEB_SEARCH_RESULT_HARD_CAP: usize = 30;
pub const WEB_SEARCH_OVERALL_TIMEOUT_SECS: u64 = 60;
pub const WEB_SEARCH_PER_ENGINE_TIMEOUT_SECS: u64 = 18;
pub const WEB_SEARCH_PER_ENGINE_RETRY_TIMEOUT_SECS: u64 = 25;
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

        let ctx = SearchContext {
            query,
            limit,
            locale,
            time_range,
            safe_search,
            category,
            timeout: Duration::from_secs(self.timeout_secs),
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
    [
        "captcha",
        "blocked the request with a captcha",
        "è¯·è¾å¥éªè¯ç ",
        "ç¾åº¦å®å¨éªè¯",
        "å®å¨éªè¯",
        "äººæºéªè¯",
        "robot check",
        "are you human",
        "verify you are not a robot",
    ]
    .iter()
    .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
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
) -> anyhow::Result<Vec<SearchHit>> {
    let first_budget = Duration::from_secs(WEB_SEARCH_PER_ENGINE_TIMEOUT_SECS);
    let retry_budget = Duration::from_secs(WEB_SEARCH_PER_ENGINE_RETRY_TIMEOUT_SECS);
    let first = match tokio::time::timeout(first_budget, engine.search(ctx)).await {
        Ok(res) => res,
        Err(_) => Err(anyhow::anyhow!(
            "engine timeout after {}s",
            WEB_SEARCH_PER_ENGINE_TIMEOUT_SECS
        )),
    };
    match first {
        Ok(hits) => Ok(hits),
        Err(err) => {
            let msg = err.to_string();
            if is_captcha_block_error(&msg) {
                tracing::info!(
                    target: "tools.web_search",
                    engine = engine.id(),
                    "engine triggered captcha/anti-bot; skipping retry"
                );
                Err(err)
            } else if is_transient_network_error(&msg) || msg.contains("engine timeout after") {
                tokio::time::sleep(Duration::from_millis(250)).await;
                tracing::debug!(
                    target: "tools.web_search",
                    engine = engine.id(),
                    error = %msg,
                    "engine returned transient network error; retrying once after 250ms"
                );
                let second = match tokio::time::timeout(retry_budget, engine.search(ctx)).await {
                    Ok(res) => res,
                    Err(_) => Err(anyhow::anyhow!(
                        "engine timeout after {}s (retry)",
                        WEB_SEARCH_PER_ENGINE_RETRY_TIMEOUT_SECS
                    )),
                };
                match second {
                    Ok(hits) => Ok(hits),
                    Err(err2) => {
                        let msg2 = err2.to_string();
                        if is_transient_network_error(&msg2) || msg2.contains("engine timeout after") {
                            tokio::time::sleep(Duration::from_millis(600)).await;
                            match tokio::time::timeout(retry_budget, engine.search(ctx)).await {
                                Ok(res) => res,
                                Err(_) => Err(anyhow::anyhow!(
                                    "engine timeout after {}s (retry 2)",
                                    WEB_SEARCH_PER_ENGINE_RETRY_TIMEOUT_SECS
                                )),
                            }
                        } else {
                            Err(err2)
                        }
                    }
                }
            } else {
                Err(err)
            }
        }
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
        "Search the web across many engines in parallel (DuckDuckGo/Bing/Brave/SearXNG/Baidu/Jina/Tavily/Exa/Serper/Google Scholar/PubMed/arXiv/Semantic Scholar/OpenAlex/Crossref/DBLP/CSDN/Juejin/Weixin/Zhihu/GitHub). Supports category (web/academic/code/cn/news/social), time_range, locale. The tool ALWAYS fans out to all available engines (up to 12 in the first concurrent wave, then a second parallel overflow wave of any remaining engines if the first wave didn't reach 8 hits); each individual engine is bounded by an 18s timeout (25s on retry) with a 60s overall budget, so slow engines never starve fast ones. When the query contains Chinese characters the chain auto-promotes Baidu/Bing/Jina/CSDN/Juejin/Weixin/Zhihu to the front so China-hosted networks (where DuckDuckGo/Brave are blocked) still get results. Results are interleaved round-robin, deduped by URL, and (for category=academic) further merged across providers by DOI / arXiv ID / normalized title so the same paper from arXiv + OpenAlex + Semantic Scholar collapses into a single richer entry. Engines that fail (captcha/transient network/timeout) are silently tolerated â?the tool returns success as long as at least one engine returns something. If every engine returns zero, the tool still returns success=true with a soft-empty diagnostic (NOT a hard error) so the agent can re-issue a reworded query without treating it as a fatal tool failure."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query." },
                "engine": {
                    "type": "string",
                    "description": "Optional preferred engine id (e.g. duckduckgo, bing, brave, searxng, baidu, jina, tavily, exa, serper, google_scholar, pubmed, arxiv, semantic_scholar, openalex, crossref, dblp, csdn, juejin, weixin, zhihu, github)."
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
                "multi": { "type": "boolean", "description": "Legacy hint; the tool now always fans out to all available engines (up to 12 in the first wave + parallel overflow) regardless of this value. Keep default true." },
                "safe_search": { "type": "boolean", "description": "Enable safe search filtering (default true)." }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let (merged, diag) = self.search_hits_internal(&args).await?;
        let mut output = String::new();
        if !merged.is_empty() && !diag.successful_engines.is_empty() {
            output.push_str(&format!(
                "Sources: {} engine(s) returned {} aggregated result(s) â?{}\n\n",
                diag.successful_engines.len(),
                merged.len(),
                diag.successful_engines.join(", ")
            ));
        }
        output.push_str(&render_results_markdown(&diag.query, &merged));
        if merged.is_empty() {
            output.push_str(
                "\n\nNo usable results returned for this query. Try rephrasing, dropping date qualifiers, \
                 setting category=`cn` or `news` explicitly, or splitting into 2-3 simpler sub-queries and \
                 calling web_search again. This is NOT a hard tool failure.",
            );
        }
        if let Some(envelope) = render_web_search_envelope(&diag, &merged) {
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

fn render_web_search_envelope(diag: &SearchDiagnostics, merged: &[SearchHit]) -> Option<String> {
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
    let payload = serde_json::json!({
        "query": diag.query,
        "successful_engines": diag.successful_engines,
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

        let fanout = chain.len().clamp(1, WEB_SEARCH_MAX_FANOUT);
        let primary_engines: Vec<Arc<_>> = chain.iter().take(fanout).cloned().collect();
        let per_engine_limit = compute_per_engine_limit(ctx.limit, primary_engines.len());

        let mut handles = Vec::new();
        for engine in primary_engines.iter() {
            let engine = engine.clone();
            let mut ctx_for_engine = ctx.clone();
            ctx_for_engine.limit = per_engine_limit;
            handles.push(tokio::spawn(async move {
                let res = run_engine_with_retry(&engine, &ctx_for_engine).await;
                (engine.label().to_string(), engine.id().to_string(), res)
            }));
        }

        let mut streams: Vec<Vec<SearchHit>> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut empty_engines: Vec<String> = Vec::new();
        let mut captcha_blocked: Vec<String> = Vec::new();
        let mut successful_engines: Vec<String> = Vec::new();

        let overall_timeout = Duration::from_secs(WEB_SEARCH_OVERALL_TIMEOUT_SECS);
        let collect_all = async {
            let mut out: Vec<(String, String, anyhow::Result<Vec<SearchHit>>)> = Vec::new();
            for handle in handles {
                match handle.await {
                    Ok(t) => out.push(t),
                    Err(e) => {
                        out.push((
                            "(join)".to_string(),
                            "join".to_string(),
                            Err(anyhow::anyhow!("join error: {e}")),
                        ));
                    }
                }
            }
            out
        };
        let collected = match tokio::time::timeout(overall_timeout, collect_all).await {
            Ok(results) => results,
            Err(_) => {
                tracing::warn!(
                    target: "tools.web_search",
                    "overall search timeout after {}s; falling back to partial results",
                    WEB_SEARCH_OVERALL_TIMEOUT_SECS
                );
                errors.push(format!(
                    "(overall timeout after {}s; partial results)",
                    WEB_SEARCH_OVERALL_TIMEOUT_SECS
                ));
                Vec::new()
            }
        };

        for (label, _id, result) in collected {
            match result {
                Ok(hits) if !hits.is_empty() => {
                    successful_engines.push(label);
                    streams.push(hits);
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

        let current_total: usize = streams.iter().map(|s| s.len()).sum();
        if current_total < WEB_SEARCH_MIN_RESULTS_TARGET && chain.len() > fanout {
            let used_ids: std::collections::HashSet<String> = primary_engines
                .iter()
                .map(|e| e.id().to_string())
                .collect();
            let overflow_engines: Vec<Arc<_>> = chain
                .iter()
                .skip(fanout)
                .filter(|e| !used_ids.contains(e.id()))
                .filter(|e| !captcha_blocked.contains(&e.label().to_string()))
                .cloned()
                .collect();
            if !overflow_engines.is_empty() {
                let overflow_ctx = ctx.clone();
                let mut overflow_handles = Vec::new();
                for engine in overflow_engines.iter() {
                    let engine = engine.clone();
                    let mut ctx_for_engine = overflow_ctx.clone();
                    ctx_for_engine.limit = per_engine_limit;
                    overflow_handles.push(tokio::spawn(async move {
                        let res = run_engine_with_retry(&engine, &ctx_for_engine).await;
                        (engine.label().to_string(), engine.id().to_string(), res)
                    }));
                }
                let collect_overflow = async {
                    let mut out = Vec::new();
                    for handle in overflow_handles {
                        if let Ok(t) = handle.await {
                            out.push(t);
                        }
                    }
                    out
                };
                let overflow_timeout =
                    Duration::from_secs(WEB_SEARCH_PER_ENGINE_RETRY_TIMEOUT_SECS + 5);
                let overflow_results = tokio::time::timeout(overflow_timeout, collect_overflow)
                    .await
                    .unwrap_or_default();
                for (label, _id, result) in overflow_results {
                    match result {
                        Ok(hits) if !hits.is_empty() => {
                            successful_engines.push(label);
                            streams.push(hits);
                        }
                        Ok(_) => empty_engines.push(label),
                        Err(e) => {
                            let msg = e.to_string();
                            if is_captcha_block_error(&msg) {
                                captcha_blocked.push(label.clone());
                            }
                            errors.push(format!("{label}: {msg}"));
                        }
                    }
                }
            }
        }

        let interleaved = merge_and_dedup(streams, ctx.limit.saturating_mul(2));
        let consolidated = if matches!(ctx.category, SearchCategory::Academic) {
            academic_merge(interleaved)
        } else {
            interleaved
        };
        let filtered = filter_by_relevance(consolidated, &ctx.query);
        let merged: Vec<SearchHit> = filtered.into_iter().take(ctx.limit).collect();
        if merged.is_empty() {
            tracing::warn!(
                target: "tools.web_search",
                query = %ctx.query,
                tried = chain.len(),
                successful = ?successful_engines,
                "all engines returned zero usable results; surfacing soft-empty payload"
            );
        } else {
            tracing::info!(
                target: "tools.web_search",
                query = %ctx.query,
                hits = merged.len(),
                successful = ?successful_engines,
                "web search ok"
            );
        }

        let diag = SearchDiagnostics {
            query: ctx.query.clone(),
            primary_engines: primary_engines
                .iter()
                .map(|e| e.label().to_string())
                .collect(),
            successful_engines,
            empty_engines,
            errors,
            captcha_blocked,
        };
        Ok((merged, diag))
    }
}
