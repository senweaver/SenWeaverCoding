// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::config::schema::FirecrawlConfig;
use crate::security::SecurityPolicy;
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

const FIRECRAWL_MIN_BODY_LEN: usize = 100;

const JINA_READER_BASE: &str = "https://r.jina.ai/";

const FETCH_TOTAL_BUDGET_MIN_SECS: u64 = 10;

const FETCH_TOTAL_BUDGET_MAX_SECS: u64 = 40;

const FETCH_HTTP_BUDGET_SECS: u64 = 8;

const FETCH_WEBVIEW_BUDGET_SECS: u64 = 15;

const FETCH_JINA_BUDGET_SECS: u64 = 10;

const FETCH_FIRECRAWL_BUDGET_SECS: u64 = 20;

const JINA_COOLDOWN: Duration = Duration::from_secs(300);

const URL_CACHE_TTL: Duration = Duration::from_secs(600);

const URL_CACHE_MAX_ENTRIES: usize = 32;

const URL_CACHE_MAX_ENTRY_BYTES: usize = 256 * 1024;

const DNS_VERDICT_TTL: Duration = Duration::from_secs(60);

const DNS_CACHE_MAX_ENTRIES: usize = 256;

static JINA_COOLDOWN_UNTIL: once_cell::sync::Lazy<parking_lot::Mutex<Option<std::time::Instant>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

fn jina_cooling_down() -> bool {
    JINA_COOLDOWN_UNTIL
        .lock()
        .is_some_and(|until| std::time::Instant::now() < until)
}

fn set_jina_cooldown() {
    *JINA_COOLDOWN_UNTIL.lock() = Some(std::time::Instant::now() + JINA_COOLDOWN);
    tracing::info!(
        target: "tools.web_fetch",
        cooldown_secs = JINA_COOLDOWN.as_secs(),
        "Jina Reader unreachable; cooling down"
    );
}

static URL_RESULT_CACHE: once_cell::sync::Lazy<
    parking_lot::Mutex<std::collections::HashMap<String, (std::time::Instant, String)>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn cached_fetch_output(url: &str) -> Option<String> {
    let cache = URL_RESULT_CACHE.lock();
    cache.get(url).and_then(|(stored_at, output)| {
        if stored_at.elapsed() < URL_CACHE_TTL {
            Some(output.clone())
        } else {
            None
        }
    })
}

fn store_fetch_output(url: &str, output: &str) {
    if output.len() > URL_CACHE_MAX_ENTRY_BYTES || output.trim().is_empty() {
        return;
    }
    let mut cache = URL_RESULT_CACHE.lock();
    if cache.len() >= URL_CACHE_MAX_ENTRIES && !cache.contains_key(url) {
        cache.retain(|_, (stored_at, _)| stored_at.elapsed() < URL_CACHE_TTL);
        if cache.len() >= URL_CACHE_MAX_ENTRIES {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (stored_at, _))| *stored_at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest);
            }
        }
    }
    cache.insert(url.to_string(), (std::time::Instant::now(), output.to_string()));
}

static DNS_VERDICT_CACHE: once_cell::sync::Lazy<
    parking_lot::Mutex<
        std::collections::HashMap<String, (std::time::Instant, Result<(), String>)>,
    >,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn cached_validate_resolved_host_is_public(host: &str) -> anyhow::Result<()> {
    {
        let cache = DNS_VERDICT_CACHE.lock();
        if let Some((checked_at, verdict)) = cache.get(host) {
            if checked_at.elapsed() < DNS_VERDICT_TTL {
                return verdict
                    .clone()
                    .map_err(|e| anyhow::anyhow!(e));
            }
        }
    }
    let verdict = validate_resolved_host_is_public(host).map_err(|e| e.to_string());
    {
        let mut cache = DNS_VERDICT_CACHE.lock();
        if cache.len() >= DNS_CACHE_MAX_ENTRIES && !cache.contains_key(host) {
            cache.retain(|_, (checked_at, _)| checked_at.elapsed() < DNS_VERDICT_TTL);
            if cache.len() >= DNS_CACHE_MAX_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(
            host.to_string(),
            (std::time::Instant::now(), verdict.clone()),
        );
    }
    verdict.map_err(|e| anyhow::anyhow!(e))
}

#[derive(Debug, Clone, Default)]
pub struct FetchedPage {
    pub url: String,
    pub title: String,
    pub text: String,
}

#[async_trait]
pub trait FetchController: Send + Sync {
    async fn fetch(&self, url: &str, timeout: Duration) -> Result<FetchedPage>;
}

static FETCH_CONTROLLER: OnceLock<Arc<dyn FetchController>> = OnceLock::new();

pub fn install_fetch_controller(controller: Arc<dyn FetchController>) {
    let _ = FETCH_CONTROLLER.set(controller);
}

pub fn fetch_controller() -> Option<Arc<dyn FetchController>> {
    FETCH_CONTROLLER.get().cloned()
}

pub fn looks_like_anti_bot_page(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    if text.len() > 8_000 {
        return false;
    }
    const NEEDLES: &[&str] = &[


        "百度安全验证",
        "请输入验证码",
        "安全验证",
        "人机验证",


        "这里空空如也",
        "娌℃湁鏇村淇℃伅",
        "暂无数据",
        "暂无内容",



        "Page Not Found",
        "Please verify",
        "Just a moment",
        "Checking your browser",
        "Access denied",
        "Access Denied",
        "Cloudflare Ray ID",
        "Bot detection",
        "captcha",
        "Captcha",
        "CAPTCHA",
        "Forbidden",


        "enable JavaScript",
        "JavaScript is required",
        "JavaScript is disabled",
        "requires JavaScript",
        "启用JavaScript",
        "启用 JavaScript",
        "开启JavaScript",
        "开启 JavaScript",
    ];
    let lower = text.to_lowercase();
    NEEDLES.iter().any(|n| {
        let needle = n.to_lowercase();
        lower.contains(&needle)
    })
}

pub struct WebFetchTool {
    security: Arc<SecurityPolicy>,
    allowed_domains: Vec<String>,
    blocked_domains: Vec<String>,
    allowed_private_hosts: Vec<String>,
    max_response_size: usize,
    timeout_secs: u64,
    firecrawl: FirecrawlConfig,
    client: std::sync::OnceLock<reqwest::Client>,
}

impl WebFetchTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        allowed_domains: Vec<String>,
        blocked_domains: Vec<String>,
        max_response_size: usize,
        timeout_secs: u64,
        firecrawl: FirecrawlConfig,
        allowed_private_hosts: Vec<String>,
    ) -> Self {
        Self {
            security,
            allowed_domains: normalize_allowed_domains(allowed_domains),
            blocked_domains: normalize_allowed_domains(blocked_domains),
            allowed_private_hosts: normalize_allowed_domains(allowed_private_hosts),
            max_response_size,
            timeout_secs,
            firecrawl,
            client: std::sync::OnceLock::new(),
        }
    }

    fn http_client(&self) -> anyhow::Result<reqwest::Client> {
        if let Some(existing) = self.client.get() {
            return Ok(existing.clone());
        }
        let timeout_secs = if self.timeout_secs == 0 {
            60
        } else {
            self.timeout_secs
        };
        let allowed_domains = self.allowed_domains.clone();
        let blocked_domains = self.blocked_domains.clone();
        let allowed_private_hosts = self.allowed_private_hosts.clone();
        let redirect_policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error(std::io::Error::other("Too many redirects (max 10)"));
            }
            if let Err(err) = validate_target_url(
                attempt.url().as_str(),
                &allowed_domains,
                &blocked_domains,
                &allowed_private_hosts,
                "web_fetch",
            ) {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Blocked redirect target: {err}"),
                ));
            }
            attempt.follow()
        });
        let builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(5))
            .redirect(redirect_policy)
            .cookie_store(true)
            .user_agent(BROWSER_USER_AGENT);
        let built = match crate::services::try_get_services() {
            Some(services) => services
                .proxy_runtime()
                .apply_to_builder(builder, "tool.web_fetch")
                .build()
                .map_err(|e| anyhow::anyhow!("web_fetch client build failed: {e}"))?,
            None => {
                return Err(anyhow::anyhow!(
                    "web_fetch blocked: service container unavailable (fail-closed)"
                ));
            }
        };
        let _ = self.client.set(built.clone());
        Ok(self.client.get().cloned().unwrap_or(built))
    }

    fn truncate_response(&self, text: &str) -> String {
        if text.len() > self.max_response_size {
            let mut truncated = text
                .chars()
                .take(self.max_response_size)
                .collect::<String>();
            truncated.push_str("\n\n... [Response truncated due to size limit] ...");
            truncated
        } else {
            text.to_string()
        }
    }

    async fn read_response_bytes_limited(
        &self,
        response: reqwest::Response,
    ) -> anyhow::Result<(Vec<u8>, Option<String>)> {
        let charset_from_header = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_charset_from_content_type);

        let mut bytes_stream = response.bytes_stream();
        let hard_cap = self.max_response_size.saturating_add(1);
        let mut bytes = Vec::new();

        while let Some(chunk_result) = bytes_stream.next().await {
            let chunk = chunk_result?;
            if append_chunk_with_cap(&mut bytes, &chunk, hard_cap) {
                break;
            }
        }

        Ok((bytes, charset_from_header))
    }

    async fn read_response_text_limited(
        &self,
        response: reqwest::Response,
    ) -> anyhow::Result<String> {
        let (bytes, charset_hint) = self.read_response_bytes_limited(response).await?;
        Ok(decode_response_bytes(&bytes, charset_hint.as_deref()))
    }
    fn should_fallback_to_jina(&self, result: &ToolResult) -> bool {
        if !result.success {
            return true;
        }
        if result.output.trim().len() < FIRECRAWL_MIN_BODY_LEN {
            return true;
        }
        if looks_like_anti_bot_page(&result.output) {
            return true;
        }
        false
    }

    async fn fetch_via_jina_reader(&self, url: &str) -> anyhow::Result<ToolResult> {
        let jina_url = format!("{}{}", JINA_READER_BASE, url);

        let client = crate::services::try_get_services()
            .ok_or_else(|| {
                anyhow::anyhow!("web_fetch blocked: service container unavailable (fail-closed)")
            })?
            .proxy_runtime()
            .build_client_with_timeouts("tool.web_fetch", FETCH_JINA_BUDGET_SECS, 5);

        let response = client
            .get(&jina_url)
            .header("Accept", "text/markdown")
            .header("User-Agent", "SenWeaverCoding/1.0")
            .send()
            .await
            .map_err(|e| {
                set_jina_cooldown();
                anyhow::anyhow!("Jina Reader request failed: {e}")
            })?;

        let status = response.status();
        if !status.is_success() {
            let code = status.as_u16();
            if status.is_server_error() || code == 429 || code == 402 {
                set_jina_cooldown();
            }
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Jina Reader error: HTTP {code}")),
            });
        }

        let body = self.read_response_text_limited(response).await?;

        if body.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Jina Reader returned empty content".into()),
            });
        }

        let output = self.truncate_response(&body);

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }

    async fn fetch_via_firecrawl(&self, url: &str) -> anyhow::Result<ToolResult> {
        let api_key = std::env::var(&self.firecrawl.api_key_env).map_err(|_| {
            anyhow::anyhow!(
                "Firecrawl API key not found in environment variable '{}'",
                self.firecrawl.api_key_env
            )
        })?;

        let endpoint = format!("{}/scrape", self.firecrawl.api_url.trim_end_matches('/'));

        let client = crate::services::try_get_services()
            .ok_or_else(|| {
                anyhow::anyhow!("web_fetch blocked: service container unavailable (fail-closed)")
            })?
            .proxy_runtime()
            .build_client_with_timeouts("tool.web_fetch", FETCH_FIRECRAWL_BUDGET_SECS, 8);

        let body = json!({
            "url": url,
            "formats": ["markdown"]
        });

        let response = client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Firecrawl request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Firecrawl API error: HTTP {} - {}",
                    status.as_u16(),
                    error_body
                )),
            });
        }

        let resp_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse Firecrawl response: {e}"))?;

        let markdown = resp_json
            .get("data")
            .and_then(|d| d.get("markdown"))
            .and_then(|m| m.as_str())
            .unwrap_or("");

        if markdown.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Firecrawl returned empty markdown content".into()),
            });
        }

        let output = self.truncate_response(markdown);

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }

    async fn standard_fetch(&self, client: &reqwest::Client, url: &str) -> ToolResult {
        let referer = referer_for(url);
        let mut req = client
            .get(url)
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
            )
            .header(
                reqwest::header::ACCEPT_LANGUAGE,
                "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7",
            )
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .header(reqwest::header::PRAGMA, "no-cache")
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "none")
            .header("Sec-Fetch-User", "?1")
            .header("Upgrade-Insecure-Requests", "1");
        if let Some(referer) = referer {
            req = req.header(reqwest::header::REFERER, referer);
        }

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("HTTP request failed: {e}")),
                };
            }
        };

        let status = response.status();
        if !status.is_success() {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "HTTP {} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Unknown")
                )),
            };
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let body_mode = if content_type.contains("text/html") || content_type.is_empty() {
            "html"
        } else if content_type.contains("text/plain")
            || content_type.contains("text/markdown")
            || content_type.contains("application/json")
        {
            "plain"
        } else {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unsupported content type: {content_type}. \
                     web_fetch supports text/html, text/plain, text/markdown, and application/json."
                )),
            };
        };

        let (raw_bytes, charset_hint) = match self
            .read_response_bytes_limited(response)
            .await
        {
            Ok(pair) => pair,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read response body: {e}")),
                };
            }
        };

        let text = if body_mode == "html" {
            let body = decode_html_bytes(&raw_bytes, charset_hint.as_deref());
            nanohtml2text::html2text(&body)
        } else {
            decode_response_bytes(&raw_bytes, charset_hint.as_deref())
        };

        let output = self.truncate_response(&text);

        ToolResult {
            success: true,
            output,
            error: None,
        }
    }
}

fn referer_for(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    if lower.contains("baidu.com/link") || lower.contains("baidu.com/s?") {
        return Some("https://www.baidu.com/");
    }
    if lower.contains("bing.com/") {
        return Some("https://www.bing.com/");
    }
    if lower.contains("sogou.com/link") {
        return Some("https://www.sogou.com/");
    }
    None
}

fn parse_charset_from_content_type(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let key = "charset=";
    let idx = lower.find(key)?;
    let rest = &value[idx + key.len()..];
    let ended = rest
        .split(|c: char| c == ';' || c.is_whitespace())
        .next()?;
    let trimmed = ended.trim_matches(|c: char| c == '"' || c == '\'').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_charset_from_html_meta(bytes: &[u8]) -> Option<String> {
    let head_len = bytes.len().min(8 * 1024);
    let head = String::from_utf8_lossy(&bytes[..head_len]);
    let lower = head.to_ascii_lowercase();
    if let Some(idx) = lower.find("charset=") {
        let rest = &head[idx + "charset=".len()..];
        let ended = rest
            .split(|c: char| {
                c == '"' || c == '\'' || c == ';' || c == '/' || c == '>' || c.is_whitespace()
            })
            .next()
            .unwrap_or("");
        let trimmed = ended.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn decode_html_bytes(bytes: &[u8], charset_hint: Option<&str>) -> String {
    let from_meta = parse_charset_from_html_meta(bytes);
    let charset = charset_hint
        .map(|s| s.to_string())
        .or(from_meta);
    decode_response_bytes(bytes, charset.as_deref())
}

fn decode_response_bytes(bytes: &[u8], charset: Option<&str>) -> String {
    let trimmed = charset
        .map(|c| c.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if trimmed.is_empty() || trimmed == "utf-8" || trimmed == "utf8" {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    if let Some(enc) = encoding_rs::Encoding::for_label(trimmed.as_bytes()) {
        let (cow, _, _) = enc.decode(bytes);
        return cow.into_owned();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a web page and return its content as clean plain text. \
         HTML pages are automatically converted to readable text; JSON and plain text \
         responses are returned as-is. Only GET requests; follows redirects. \
         Fast by design: a direct HTTP fetch (8s budget) races a desktop webview fetch \
         (15s budget, JS-capable) and the first good result wins, typically in 1-3s for \
         static pages; Jina Reader (10s) and Firecrawl (20s, requires API key) run only \
         as fallbacks within a 10-40s total deadline. Recently fetched URLs are served \
         from a 10-minute cache. If the result reports a connectivity problem (network/\
         proxy), do NOT retry immediately - inform the user instead. \
         Security: allowlist-only domains, no local/private hosts."
    }

    fn mcp_safe(&self) -> bool {

        true
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The HTTP or HTTPS URL to fetch"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let url_hint = args
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("web_fetch")
            .to_string();
        let mut result = self.fetch_to_result(args).await?;
        if result.success {
            result.output =
                crate::security::prompt_guard::core::PromptGuard::screen_untrusted_web_content(
                    &url_hint,
                    std::mem::take(&mut result.output),
                );
        }
        if result.success && crate::token_saver::is_enabled() {
            result.output = crate::token_saver::compact_tool_output(
                "web_fetch",
                &result.output,
                &crate::token_saver::global(),
            );
        }
        Ok(result)
    }
}

enum WebviewRace {
    Unavailable,
    Fetched(ToolResult),
    Failed(String),
}

impl WebFetchTool {
    async fn fetch_to_result(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let raw_url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?
            .to_string();

        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: rate limit exceeded".into()),
            });
        }

        let url = {
            let allowed = self.allowed_domains.clone();
            let blocked = self.blocked_domains.clone();
            let private_hosts = self.allowed_private_hosts.clone();
            let candidate = raw_url.clone();
            match tokio::task::spawn_blocking(move || {
                validate_target_url(&candidate, &allowed, &blocked, &private_hosts, "web_fetch")
            })
            .await
            {
                Ok(Ok(validated)) => validated,
                Ok(Err(e)) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(e.to_string()),
                    });
                }
                Err(join_err) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("URL validation task failed: {join_err}")),
                    });
                }
            }
        };

        if let Some(cached) = cached_fetch_output(&url) {
            tracing::debug!(
                target: "tools.web_fetch",
                url = %url,
                "returning cached fetch result"
            );
            return Ok(ToolResult {
                success: true,
                output: cached,
                error: None,
            });
        }

        let total_secs = if self.timeout_secs == 0 { 30 } else { self.timeout_secs }
            .clamp(FETCH_TOTAL_BUDGET_MIN_SECS, FETCH_TOTAL_BUDGET_MAX_SECS);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(total_secs);
        let budget_capped = |cap_secs: u64| -> Duration {
            Duration::from_secs(cap_secs)
                .min(deadline.saturating_duration_since(tokio::time::Instant::now()))
        };

        let client = self.http_client()?;

        let http_fut = async {
            let budget = budget_capped(FETCH_HTTP_BUDGET_SECS);
            match tokio::time::timeout(budget, self.standard_fetch(&client, &url)).await {
                Ok(res) => res,
                Err(_) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "standard fetch timeout after {:.1}s",
                        budget.as_secs_f32()
                    )),
                },
            }
        };
        let webview_fut = async {
            let Some(controller) = fetch_controller() else {
                return WebviewRace::Unavailable;
            };
            let budget = budget_capped(FETCH_WEBVIEW_BUDGET_SECS);
            match tokio::time::timeout(budget, controller.fetch(&url, budget)).await {
                Ok(Ok(page)) => WebviewRace::Fetched(ToolResult {
                    success: true,
                    output: self.truncate_response(&page.text),
                    error: None,
                }),
                Ok(Err(e)) => WebviewRace::Failed(e.to_string()),
                Err(_) => WebviewRace::Failed(format!(
                    "webview fetch timeout after {:.1}s",
                    budget.as_secs_f32()
                )),
            }
        };
        tokio::pin!(http_fut);
        tokio::pin!(webview_fut);

        let mut standard_result: Option<ToolResult> = None;
        let mut webview_candidate: Option<ToolResult> = None;
        let mut webview_error: Option<String> = None;
        let mut webview_finished = false;

        loop {
            tokio::select! {
                res = &mut http_fut, if standard_result.is_none() => {
                    if !self.should_fallback_to_jina(&res) {
                        store_fetch_output(&url, &res.output);
                        return Ok(res);
                    }
                    standard_result = Some(res);
                    if webview_finished {
                        break;
                    }
                }
                race = &mut webview_fut, if !webview_finished => {
                    webview_finished = true;
                    match race {
                        WebviewRace::Unavailable => {}
                        WebviewRace::Fetched(candidate) => {
                            if !self.should_fallback_to_jina(&candidate) {
                                store_fetch_output(&url, &candidate.output);
                                return Ok(candidate);
                            }
                            tracing::info!(
                                "web_fetch: webview fetch returned likely anti-bot or empty content for {url}"
                            );
                            webview_candidate = Some(candidate);
                        }
                        WebviewRace::Failed(msg) => {
                            tracing::warn!(
                                "web_fetch: webview fetch failed for {url}: {msg}; relying on HTTP path"
                            );
                            webview_error = Some(msg);
                        }
                    }
                    if standard_result.is_some() {
                        break;
                    }
                }
            }
        }
        let standard_result = standard_result.unwrap_or_else(|| ToolResult {
            success: false,
            output: String::new(),
            error: Some("standard fetch did not complete".into()),
        });

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining >= Duration::from_secs(2) && !jina_cooling_down() {
            tracing::info!(
                "web_fetch: direct paths insufficient for {url}, attempting Jina Reader fallback"
            );
            let budget = budget_capped(FETCH_JINA_BUDGET_SECS);
            match tokio::time::timeout(budget, Box::pin(self.fetch_via_jina_reader(&url))).await {
                Ok(Ok(jina_result)) if jina_result.success => {
                    store_fetch_output(&url, &jina_result.output);
                    return Ok(jina_result);
                }
                Ok(Ok(jina_result)) => {
                    tracing::warn!(
                        "web_fetch: Jina Reader fallback failed: {:?}",
                        jina_result.error
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!("web_fetch: Jina Reader fallback error: {e}");
                }
                Err(_) => {
                    set_jina_cooldown();
                    tracing::warn!(
                        "web_fetch: Jina Reader timed out after {:.1}s",
                        budget.as_secs_f32()
                    );
                }
            }
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if self.firecrawl.enabled && remaining >= Duration::from_secs(3) {
            tracing::info!(
                "web_fetch: attempting Firecrawl fallback for {url}"
            );
            let budget = budget_capped(FETCH_FIRECRAWL_BUDGET_SECS);
            match tokio::time::timeout(budget, Box::pin(self.fetch_via_firecrawl(&url))).await {
                Ok(Ok(firecrawl_result)) if firecrawl_result.success => {
                    store_fetch_output(&url, &firecrawl_result.output);
                    return Ok(firecrawl_result);
                }
                Ok(Ok(firecrawl_result)) => {
                    tracing::warn!(
                        "web_fetch: Firecrawl fallback also failed: {:?}",
                        firecrawl_result.error
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!("web_fetch: Firecrawl fallback error: {e}");
                }
                Err(_) => {
                    tracing::warn!(
                        "web_fetch: Firecrawl timed out after {:.1}s",
                        budget.as_secs_f32()
                    );
                }
            }
        }

        let best = pick_best_result(webview_candidate, standard_result, webview_error);
        if best.success {
            store_fetch_output(&url, &best.output);
        }
        Ok(best)
    }
}

fn result_quality_score(result: &ToolResult) -> usize {
    if !result.success {
        return 0;
    }
    let len = result.output.trim().chars().count();
    if len == 0 {
        return 0;
    }
    if looks_like_anti_bot_page(&result.output) {
        return len.min(50);
    }
    len.saturating_add(1_000_000)
}

fn pick_best_result(
    webview: Option<ToolResult>,
    standard: ToolResult,
    webview_error: Option<String>,
) -> ToolResult {
    let standard_score = result_quality_score(&standard);
    let webview_score = webview
        .as_ref()
        .map(result_quality_score)
        .unwrap_or(0);

    let standard_error_for_diag = standard.error.clone();
    let best = if webview_score > standard_score {
        webview.unwrap_or(standard)
    } else {
        standard
    };

    if best.success && looks_like_anti_bot_page(&best.output) {
        let head: String = best.output.chars().take(120).collect();
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!(
                "All fetch paths returned an anti-bot or empty page. \
                 Snippet of last response: {head}\n\
                 目标站点触发了反爬/人机验证,无法获取正文;请稍后重试或改用其它来源链接。"
            )),
        };
    }
    if best.success && best.output.trim().chars().count() < 16 {
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some(
                "All fetch paths returned an empty or near-empty page. \
                 The target may be deleted, paywalled, or login-required.\n\
                 页面为空或近乎为空:可能已被删除、需要登录或付费;请更换来源链接。"
                    .into(),
            ),
        };
    }
    if !best.success {
        let mut parts: Vec<String> = Vec::new();
        if let Some(err) = best.error.as_ref() {
            parts.push(format!("standard fetch: {err}"));
        } else if let Some(err) = standard_error_for_diag.as_ref() {
            parts.push(format!("standard fetch: {err}"));
        }
        if let Some(err) = webview_error.as_ref() {
            parts.push(format!("webview fetch: {err}"));
        }
        if !parts.is_empty() {
            let all_network = parts.iter().all(|p| is_network_error_text(p));
            let error = if all_network {
                format!(
                    "Unable to reach the target site: {}. This is a CONNECTIVITY problem \
                     (network, proxy or firewall), not a problem with the URL itself — do \
                     not retry immediately; inform the user and check the proxy settings \
                     (services.proxy_runtime / HTTP(S)_PROXY).\n\
                     网络连接异常:无法访问目标网址,请检查网络连接或代理设置;这不是 URL 本身的问题。",
                    parts.join("; ")
                )
            } else {
                format!(
                    "All fetch paths failed. {}. \
                     If your network requires a proxy, configure it via the proxy runtime \
                     settings (services.proxy_runtime / HTTP(S)_PROXY) and retry.\n\
                     抓取失败:所有获取路径均未成功;可稍后重试或更换来源链接。",
                    parts.join("; ")
                )
            };
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            };
        }
    }
    best
}

fn is_network_error_text(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    [
        "timeout",
        "timed out",
        "error sending request",
        "connection refused",
        "connection reset",
        "connection closed",
        "dns",
        "tls",
        "handshake",
        "unreachable",
        "broken pipe",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
                                  AppleWebKit/537.36 (KHTML, like Gecko) \
                                  Chrome/127.0.0.0 Safari/537.36 Edg/127.0.0.0";

fn validate_target_url(
    raw_url: &str,
    allowed_domains: &[String],
    blocked_domains: &[String],
    allowed_private_hosts: &[String],
    tool_name: &str,
) -> anyhow::Result<String> {
    let url = raw_url.trim();

    if url.is_empty() {
        anyhow::bail!("URL cannot be empty");
    }

    if url.chars().any(char::is_whitespace) {
        anyhow::bail!("URL cannot contain whitespace");
    }

    if !url.starts_with("http://") && !url.starts_with("https://") {
        anyhow::bail!("Only http:// and https:// URLs are allowed");
    }

    if allowed_domains.is_empty() {
        anyhow::bail!(
            "{tool_name} tool is enabled but no allowed_domains are configured. \
             Add [{tool_name}].allowed_domains in config.toml"
        );
    }

    let host = extract_host(url)?;

    if host_matches_allowlist(&host, blocked_domains) {
        anyhow::bail!("Host '{host}' is in {tool_name}.blocked_domains");
    }

    let private_host_allowed =
        is_private_or_local_host(&host) && host_matches_allowlist(&host, allowed_private_hosts);

    if is_private_or_local_host(&host) && !private_host_allowed {
        anyhow::bail!(
            "Blocked local/private host: {host}. \
             To allow this host, add it to {tool_name}.allowed_private_hosts in config.toml"
        );
    }

    if private_host_allowed {
        tracing::warn!(
            "{tool_name}: allowing private/local host '{host}' via allowed_private_hosts"
        );
    }

    if !private_host_allowed && !host_matches_allowlist(&host, allowed_domains) {
        anyhow::bail!("Host '{host}' is not in {tool_name}.allowed_domains");
    }

    if !private_host_allowed {
        cached_validate_resolved_host_is_public(&host)?;
    }

    Ok(url.to_string())
}

fn append_chunk_with_cap(buffer: &mut Vec<u8>, chunk: &[u8], hard_cap: usize) -> bool {
    if buffer.len() >= hard_cap {
        return true;
    }

    let remaining = hard_cap - buffer.len();
    if chunk.len() > remaining {
        buffer.extend_from_slice(&chunk[..remaining]);
        return true;
    }

    buffer.extend_from_slice(chunk);
    buffer.len() >= hard_cap
}

fn normalize_allowed_domains(domains: Vec<String>) -> Vec<String> {
    let mut normalized = domains
        .into_iter()
        .filter_map(|d| normalize_domain(&d))
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}

fn normalize_domain(raw: &str) -> Option<String> {
    let mut d = raw.trim().to_lowercase();
    if d.is_empty() {
        return None;
    }

    if let Some(stripped) = d.strip_prefix("https://") {
        d = stripped.to_string();
    } else if let Some(stripped) = d.strip_prefix("http://") {
        d = stripped.to_string();
    }

    if let Some((host, _)) = d.split_once('/') {
        d = host.to_string();
    }

    d = d.trim_start_matches('.').trim_end_matches('.').to_string();

    if let Some((host, _)) = d.split_once(':') {
        d = host.to_string();
    }

    if d.is_empty() || d.chars().any(char::is_whitespace) {
        return None;
    }

    Some(d)
}

pub(crate) fn extract_host(url: &str) -> anyhow::Result<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(|| anyhow::anyhow!("Only http:// and https:// URLs are allowed"))?;

    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid URL"))?;

    if authority.is_empty() {
        anyhow::bail!("URL must include a host");
    }

    if authority.contains('@') {
        anyhow::bail!("URL userinfo is not allowed");
    }

    if authority.starts_with('[') {
        anyhow::bail!("IPv6 hosts are not supported in web_fetch");
    }

    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.')
        .to_lowercase();

    if host.is_empty() {
        anyhow::bail!("URL must include a valid host");
    }

    Ok(host)
}

fn host_matches_allowlist(host: &str, allowed_domains: &[String]) -> bool {
    if allowed_domains.iter().any(|domain| domain == "*") {
        return true;
    }

    allowed_domains.iter().any(|domain| {
        host == domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

pub(crate) fn is_private_or_local_host(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);

    let has_local_tld = bare
        .rsplit('.')
        .next()
        .is_some_and(|label| label == "local");

    if bare == "localhost" || bare.ends_with(".localhost") || has_local_tld {
        return true;
    }

    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => is_non_global_v4(v4),
            std::net::IpAddr::V6(v6) => is_non_global_v6(v6),
        };
    }

    false
}

pub(crate) fn validate_resolved_host_is_public(host: &str) -> anyhow::Result<()> {
    use std::net::ToSocketAddrs;

    let ips = (host, 0)
        .to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("Failed to resolve host '{host}': {e}"))?
        .map(|addr| addr.ip())
        .collect::<Vec<_>>();

    validate_resolved_ips_are_public(host, &ips)
}

fn validate_resolved_ips_are_public(host: &str, ips: &[std::net::IpAddr]) -> anyhow::Result<()> {
    if ips.is_empty() {
        anyhow::bail!("Failed to resolve host '{host}'");
    }

    for ip in ips {
        let non_global = match ip {
            std::net::IpAddr::V4(v4) => is_non_global_v4(*v4),
            std::net::IpAddr::V6(v6) => is_non_global_v6(*v6),
        };
        if non_global {
            anyhow::bail!("Blocked host '{host}' resolved to non-global address {ip}");
        }
    }

    Ok(())
}

fn is_private_v4(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, _c, _d] = v4.octets();
    (a == 10) || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

fn is_non_global_v4(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _d] = v4.octets();
    v4.is_loopback()
        || is_private_v4(v4)
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || (a == 100 && (64..=127).contains(&b))
        || a >= 240
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 198 && b == 51)
        || (a == 203 && b == 0)
        || (a == 198 && (18..=19).contains(&b))
}

fn is_non_global_v6(v6: std::net::Ipv6Addr) -> bool {
    let segs = v6.segments();
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        || (segs[0] & 0xfe00) == 0xfc00
        || (segs[0] & 0xffc0) == 0xfe80
        || (segs[0] == 0x2001 && segs[1] == 0x0db8)
        || v6.to_ipv4_mapped().is_some_and(is_non_global_v4)
}
