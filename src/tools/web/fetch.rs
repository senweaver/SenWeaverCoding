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
            .connect_timeout(Duration::from_secs(10))
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

    fn validate_url(&self, raw_url: &str) -> anyhow::Result<String> {
        validate_target_url(
            raw_url,
            &self.allowed_domains,
            &self.blocked_domains,
            &self.allowed_private_hosts,
            "web_fetch",
        )
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
            .build_client_with_timeouts("tool.web_fetch", 30, 10);

        let response = client
            .get(&jina_url)
            .header("Accept", "text/markdown")
            .header("User-Agent", "SenWeaverCoding/1.0")
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Jina Reader request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Jina Reader error: HTTP {}", status.as_u16())),
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
            .build_client_with_timeouts("tool.web_fetch", 60, 10);

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
            .header(reqwest::header::ACCEPT_ENCODING, "gzip, deflate, br")
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
         HTML pages are automatically converted to readable text. \
         JSON and plain text responses are returned as-is. \
         Only GET requests; follows redirects. \
         Falls back to Jina Reader (free) then Firecrawl for JS-heavy/bot-blocked sites. \
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
        let mut result = self.fetch_to_result(args).await?;
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

impl WebFetchTool {
    async fn fetch_to_result(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

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

        let url = match self.validate_url(url) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                });
            }
        };

        let timeout_secs = if self.timeout_secs == 0 {
            tracing::warn!("web_fetch: timeout_secs is 0, using safe default of 60s");
            60
        } else {
            self.timeout_secs
        };

        let mut webview_candidate: Option<ToolResult> = None;
        let mut webview_error: Option<String> = None;
        if let Some(controller) = fetch_controller() {
            let webview_timeout = Duration::from_secs(timeout_secs.max(30));
            match controller.fetch(&url, webview_timeout).await {
                Ok(page) => {
                    let candidate = ToolResult {
                        success: true,
                        output: self.truncate_response(&page.text),
                        error: None,
                    };
                    if !self.should_fallback_to_jina(&candidate) {
                        return Ok(candidate);
                    }
                    tracing::info!(
                        "web_fetch: webview fetch returned likely anti-bot or empty content for {url}, falling back"
                    );
                    webview_candidate = Some(candidate);
                }
                Err(err) => {
                    let msg = err.to_string();
                    tracing::warn!(
                        "web_fetch: webview fetch failed for {url}: {msg}; falling back to HTTP path"
                    );
                    webview_error = Some(msg);
                }
            }
        }

        let client = self.http_client()?;

        let standard_result = self.standard_fetch(&client, &url).await;

        if self.should_fallback_to_jina(&standard_result) {
            tracing::info!(
                "web_fetch: standard fetch insufficient for {url}, attempting Jina Reader fallback"
            );
            match Box::pin(self.fetch_via_jina_reader(&url)).await {
                Ok(jina_result) if jina_result.success => {
                    return Ok(jina_result);
                }
                Ok(jina_result) => {
                    tracing::warn!(
                        "web_fetch: Jina Reader fallback failed: {:?}",
                        jina_result.error
                    );
                }
                Err(e) => {
                    tracing::warn!("web_fetch: Jina Reader fallback error: {e}");
                }
            }

            if self.firecrawl.enabled {
                tracing::info!(
                    "web_fetch: Jina Reader also insufficient, attempting Firecrawl fallback"
                );
                match Box::pin(self.fetch_via_firecrawl(&url)).await {
                    Ok(firecrawl_result) if firecrawl_result.success => {
                        return Ok(firecrawl_result);
                    }
                    Ok(firecrawl_result) => {
                        tracing::warn!(
                            "web_fetch: Firecrawl fallback also failed: {:?}",
                            firecrawl_result.error
                        );
                    }
                    Err(e) => {
                        tracing::warn!("web_fetch: Firecrawl fallback error: {e}");
                    }
                }
            }
        }

        let best = pick_best_result(webview_candidate, standard_result, webview_error);
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
                 Snippet of last response: {head}"
            )),
        };
    }
    if best.success && best.output.trim().chars().count() < 16 {
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some(
                "All fetch paths returned an empty or near-empty page. \
                 The target may be deleted, paywalled, or login-required."
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
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "All fetch paths failed. {}. \
                     If your network requires a proxy, configure it via the proxy runtime \
                     settings (services.proxy_runtime / HTTP(S)_PROXY) and retry.",
                    parts.join("; ")
                )),
            };
        }
    }
    best
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
        validate_resolved_host_is_public(&host)?;
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
