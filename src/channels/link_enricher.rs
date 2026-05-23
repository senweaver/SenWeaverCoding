// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use regex::Regex;
use std::net::IpAddr;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LinkEnricherConfig {
    pub enabled: bool,
    pub max_links: usize,
    pub timeout_secs: u64,
}

impl Default for LinkEnricherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_links: 3,
            timeout_secs: 10,
        }
    }
}

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"']+"#).expect("URL regex must compile"));

pub fn extract_urls(text: &str, max: usize) -> Vec<String> {
    let mut seen = Vec::new();
    for m in URL_RE.find_iter(text) {
        let url = m.as_str().to_string();
        if !seen.contains(&url) {
            seen.push(url);
            if seen.len() >= max {
                break;
            }
        }
    }
    seen
}

pub fn is_ssrf_target(url: &str) -> bool {
    let host = match extract_host(url) {
        Some(h) => h,
        None => return true,
    };

    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host == "local"
    {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(ip);
    }

    false
}

fn extract_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }

    let host = if authority.starts_with('[') {

        return None;
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    Some(host.to_lowercase())
}

fn is_private_v4(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, _c, _d] = v4.octets();
    (a == 10) || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || is_private_v4(v4)
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()

                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback()
                        || is_private_v4(v4)
                        || v4.is_link_local()
                        || v4.is_unspecified()
                })
        }
    }
}

pub fn extract_title(html: &str) -> Option<String> {

    let lower = html.to_lowercase();
    let start = lower.find("<title")? + "<title".len();

    let start = lower[start..].find('>')? + start + 1;
    let end = lower[start..].find("</title")? + start;
    let title = lower[start..end].trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(html_entity_decode_basic(&title))
    }
}

pub fn extract_body_text(html: &str, max_chars: usize) -> String {
    let text = nanohtml2text::html2text(html);
    let trimmed = text.trim();
    if trimmed.len() <= max_chars {
        trimmed.to_string()
    } else {
        let mut result: String = trimmed.chars().take(max_chars).collect();
        result.push_str("...");
        result
    }
}

fn html_entity_decode_basic(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

struct LinkSummary {
    title: String,
    snippet: String,
}

async fn fetch_link_summary(url: &str, timeout_secs: u64) -> Option<LinkSummary> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("SenWeaverCoding/0.1 (link-enricher)")
        .build()
        .ok()?;

    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    if !content_type.contains("text/html") && !content_type.is_empty() {
        return None;
    }

    let max_bytes: usize = 256 * 1024;
    let bytes = response.bytes().await.ok()?;
    let body = if bytes.len() > max_bytes {
        String::from_utf8_lossy(&bytes[..max_bytes]).into_owned()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };

    let title = extract_title(&body).unwrap_or_else(|| "Untitled".to_string());
    let snippet = extract_body_text(&body, 200);

    Some(LinkSummary { title, snippet })
}

pub async fn enrich_message(content: &str, config: &LinkEnricherConfig) -> String {
    if !config.enabled || config.max_links == 0 {
        return content.to_string();
    }

    let urls = extract_urls(content, config.max_links);
    if urls.is_empty() {
        return content.to_string();
    }

    let safe_urls: Vec<&str> = urls
        .iter()
        .filter(|u| !is_ssrf_target(u))
        .map(|u| u.as_str())
        .collect();
    if safe_urls.is_empty() {
        return content.to_string();
    }

    let mut enrichments = Vec::new();
    for url in safe_urls {
        match fetch_link_summary(url, config.timeout_secs).await {
            Some(summary) => {
                enrichments.push(format!("[Link: {} — {}]", summary.title, summary.snippet));
            }
            None => {
                tracing::debug!(url, "Link enricher: failed to fetch or extract summary");
            }
        }
    }

    if enrichments.is_empty() {
        return content.to_string();
    }

    let prefix = enrichments.join("\n");
    format!("{prefix}\n{content}")
}
