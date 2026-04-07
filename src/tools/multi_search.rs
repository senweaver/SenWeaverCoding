// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Multi-engine aggregated search with result merging and ranking.
//!
//! Inspired by Agent-Reach's multi-platform search breadth, this tool
//! queries multiple search engines in parallel and merges/de-duplicates
//! results with a scoring algorithm that combines relevance and source diversity.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;

pub struct MultiSearchTool {
    max_results: usize,
    timeout_secs: u64,
    brave_api_key: Option<String>,
    searxng_url: Option<String>,
    tavily_api_key: Option<String>,
    exa_api_key: Option<String>,
}

impl MultiSearchTool {
    pub fn new(
        max_results: usize,
        timeout_secs: u64,
        brave_api_key: Option<String>,
        searxng_url: Option<String>,
    ) -> Self {
        Self {
            max_results: max_results.clamp(1, 20),
            timeout_secs: timeout_secs.max(5),
            brave_api_key,
            searxng_url,
            tavily_api_key: std::env::var("TAVILY_API_KEY").ok(),
            exa_api_key: std::env::var("EXA_API_KEY").ok(),
        }
    }

    pub fn with_tavily_key(mut self, key: Option<String>) -> Self {
        if key.is_some() {
            self.tavily_api_key = key;
        }
        self
    }

    pub fn with_exa_key(mut self, key: Option<String>) -> Self {
        if key.is_some() {
            self.exa_api_key = key;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
    source: String,
    rank: usize,
    score: f64,
}

#[async_trait]
impl Tool for MultiSearchTool {
    fn name(&self) -> &str {
        "multi_search"
    }

    fn description(&self) -> &str {
        "Search across multiple engines simultaneously (DuckDuckGo, Brave, SearXNG, Tavily, Exa) \
         and return merged, de-duplicated, ranked results. Use for comprehensive research \
         where you want the best results from multiple sources."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum total results after merging (default 10, max 20)"
                },
                "engines": {
                    "type": "array",
                    "items": {"type": "string", "enum": ["duckduckgo", "brave", "searxng", "tavily", "exa"]},
                    "description": "Engines to query (default: all available)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        if query.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Query must not be empty".into()),
            });
        }

        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.max_results as u64) as usize;
        let max = max.clamp(1, 20);

        let requested_engines: Vec<String> = args
            .get("engines")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .user_agent("Mozilla/5.0 (compatible; SenWeaverCoding/1.0)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| anyhow::anyhow!("HTTP client error: {e}"))?;

        let use_ddg =
            requested_engines.is_empty() || requested_engines.iter().any(|e| e == "duckduckgo");
        let use_brave = (requested_engines.is_empty()
            || requested_engines.iter().any(|e| e == "brave"))
            && self.brave_api_key.is_some();
        let use_searxng = (requested_engines.is_empty()
            || requested_engines.iter().any(|e| e == "searxng"))
            && self.searxng_url.is_some();

        let mut handles = Vec::new();

        if use_ddg {
            let c = client.clone();
            let q = query.to_string();
            let mr = self.max_results;
            handles.push(tokio::spawn(
                async move { search_duckduckgo(&c, &q, mr).await },
            ));
        }

        if use_brave {
            let c = client.clone();
            let q = query.to_string();
            let key = self.brave_api_key.clone().unwrap();
            let mr = self.max_results;
            handles.push(tokio::spawn(
                async move { search_brave(&c, &q, &key, mr).await },
            ));
        }

        if use_searxng {
            let c = client.clone();
            let q = query.to_string();
            let url = self.searxng_url.clone().unwrap();
            let mr = self.max_results;
            handles.push(tokio::spawn(async move {
                search_searxng(&c, &q, &url, mr).await
            }));
        }

        let use_tavily = (requested_engines.is_empty()
            || requested_engines.iter().any(|e| e == "tavily"))
            && self.tavily_api_key.is_some();
        let use_exa = (requested_engines.is_empty()
            || requested_engines.iter().any(|e| e == "exa"))
            && self.exa_api_key.is_some();

        if use_tavily {
            let c = client.clone();
            let q = query.to_string();
            let key = self.tavily_api_key.clone().unwrap();
            let mr = self.max_results;
            handles.push(tokio::spawn(
                async move { search_tavily(&c, &q, &key, mr).await },
            ));
        }

        if use_exa {
            let c = client.clone();
            let q = query.to_string();
            let key = self.exa_api_key.clone().unwrap();
            let mr = self.max_results;
            handles.push(tokio::spawn(
                async move { search_exa(&c, &q, &key, mr).await },
            ));
        }

        let mut all_results: Vec<SearchResult> = Vec::new();
        let mut engines_used: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for handle in handles {
            match handle.await {
                Ok(Ok((source, results))) => {
                    engines_used.push(source);
                    all_results.extend(results);
                }
                Ok(Err(e)) => {
                    errors.push(e.to_string());
                }
                Err(e) => {
                    errors.push(format!("Task error: {e}"));
                }
            }
        }

        if all_results.is_empty() {
            let err_detail = if errors.is_empty() {
                "No search engines available or no results found".to_string()
            } else {
                format!("All engines failed: {}", errors.join("; "))
            };
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(err_detail),
            });
        }

        let merged = merge_and_rank(all_results, max);

        let mut output = format!(
            "Multi-search results for: {} (engines: {})\n\n",
            query,
            engines_used.join(", ")
        );

        for (i, result) in merged.iter().enumerate() {
            output.push_str(&format!(
                "{}. [{}] {}\n   {}\n",
                i + 1,
                result.source,
                result.title,
                result.url,
            ));
            if !result.snippet.is_empty() {
                output.push_str(&format!("   {}\n", result.snippet));
            }
            output.push('\n');
        }

        if !errors.is_empty() {
            output.push_str(&format!(
                "Note: {} engine(s) had errors: {}\n",
                errors.len(),
                errors.join("; ")
            ));
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

fn merge_and_rank(mut results: Vec<SearchResult>, max: usize) -> Vec<SearchResult> {
    let mut seen_urls: HashSet<String> = HashSet::new();
    results.retain(|r| {
        let normalized = normalize_url(&r.url);
        seen_urls.insert(normalized)
    });

    for result in &mut results {
        let rank_score = 1.0 / (result.rank as f64 + 1.0);
        let source_bonus = match result.source.as_str() {
            "tavily" => 0.15,
            "exa" => 0.12,
            "brave" => 0.1,
            "searxng" => 0.05,
            _ => 0.0,
        };
        let title_bonus = if !result.title.is_empty() { 0.05 } else { 0.0 };
        let snippet_bonus = if result.snippet.len() > 50 { 0.05 } else { 0.0 };

        result.score = rank_score + source_bonus + title_bonus + snippet_bonus;
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results.truncate(max);
    results
}

fn normalize_url(url: &str) -> String {
    let mut u = url.to_lowercase();
    u = u.trim_end_matches('/').to_string();
    if let Some(stripped) = u.strip_prefix("https://") {
        u = stripped.to_string();
    } else if let Some(stripped) = u.strip_prefix("http://") {
        u = stripped.to_string();
    }
    if let Some(stripped) = u.strip_prefix("www.") {
        u = stripped.to_string();
    }
    u
}

async fn search_duckduckgo(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> anyhow::Result<(String, Vec<SearchResult>)> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await?;

    let body = resp.text().await?;
    let mut results = Vec::new();

    let link_re = regex::Regex::new(r#"class="result__a"[^>]*href="([^"]*)"[^>]*>([^<]*)</a>"#)?;
    let snippet_re = regex::Regex::new(r#"class="result__snippet"[^>]*>(.*?)</a>"#)?;

    let links: Vec<(String, String)> = link_re
        .captures_iter(&body)
        .map(|cap| {
            let raw_url = cap.get(1).map_or("", |m| m.as_str()).to_string();
            let title = strip_tags(cap.get(2).map_or("", |m| m.as_str()));
            let url = decode_ddg_url(&raw_url);
            (url, title)
        })
        .collect();

    let snippets: Vec<String> = snippet_re
        .captures_iter(&body)
        .map(|cap| strip_tags(cap.get(1).map_or("", |m| m.as_str())))
        .collect();

    for (i, (url, title)) in links.iter().take(max_results).enumerate() {
        if url.is_empty() || url.starts_with("https://duckduckgo.com") {
            continue;
        }
        results.push(SearchResult {
            title: title.clone(),
            url: url.clone(),
            snippet: snippets.get(i).cloned().unwrap_or_default(),
            source: "duckduckgo".into(),
            rank: i,
            score: 0.0,
        });
    }

    Ok(("duckduckgo".into(), results))
}

async fn search_brave(
    client: &reqwest::Client,
    query: &str,
    api_key: &str,
    max_results: usize,
) -> anyhow::Result<(String, Vec<SearchResult>)> {
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoding::encode(query),
        max_results
    );

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .send()
        .await?;

    let body: serde_json::Value = resp.json().await?;
    let mut results = Vec::new();

    if let Some(web_results) = body
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array())
    {
        for (i, item) in web_results.iter().take(max_results).enumerate() {
            results.push(SearchResult {
                title: item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                url: item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                snippet: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                source: "brave".into(),
                rank: i,
                score: 0.0,
            });
        }
    }

    Ok(("brave".into(), results))
}

async fn search_searxng(
    client: &reqwest::Client,
    query: &str,
    base_url: &str,
    max_results: usize,
) -> anyhow::Result<(String, Vec<SearchResult>)> {
    let url = format!(
        "{}/search?q={}&format=json&pageno=1",
        base_url.trim_end_matches('/'),
        urlencoding::encode(query)
    );

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await?;

    let body: serde_json::Value = resp.json().await?;
    let mut results = Vec::new();

    if let Some(items) = body.get("results").and_then(|r| r.as_array()) {
        for (i, item) in items.iter().take(max_results).enumerate() {
            results.push(SearchResult {
                title: item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                url: item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                snippet: item
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                source: "searxng".into(),
                rank: i,
                score: 0.0,
            });
        }
    }

    Ok(("searxng".into(), results))
}

fn decode_ddg_url(raw: &str) -> String {
    if let Some(pos) = raw.find("uddg=") {
        let start = pos + 5;
        let end = raw[start..].find('&').unwrap_or(raw.len() - start);
        urlencoding::decode(&raw[start..start + end])
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    }
}

fn strip_tags(html: &str) -> String {
    let re = regex::Regex::new(r"<[^>]*>").unwrap();
    let text = re.replace_all(html, "");
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

async fn search_tavily(
    client: &reqwest::Client,
    query: &str,
    api_key: &str,
    max_results: usize,
) -> anyhow::Result<(String, Vec<SearchResult>)> {
    let body = serde_json::json!({
        "api_key": api_key,
        "query": query,
        "search_depth": "basic",
        "max_results": max_results,
        "include_answer": false,
    });

    let resp = client
        .post("https://api.tavily.com/search")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Tavily HTTP {}", resp.status());
    }

    #[derive(serde::Deserialize)]
    struct TavilyResp {
        #[serde(default)]
        results: Vec<TavilyItem>,
    }
    #[derive(serde::Deserialize)]
    struct TavilyItem {
        title: String,
        url: String,
        content: String,
        #[serde(default)]
        score: f64,
    }

    let data: TavilyResp = resp.json().await?;
    let results = data
        .results
        .into_iter()
        .enumerate()
        .map(|(i, r)| SearchResult {
            title: r.title,
            url: r.url,
            snippet: r.content,
            source: "tavily".to_string(),
            rank: i,
            score: r.score,
        })
        .collect();

    Ok(("tavily".to_string(), results))
}

async fn search_exa(
    client: &reqwest::Client,
    query: &str,
    api_key: &str,
    max_results: usize,
) -> anyhow::Result<(String, Vec<SearchResult>)> {
    let body = serde_json::json!({
        "query": query,
        "type": "neural",
        "numResults": max_results,
    });

    let resp = client
        .post("https://api.exa.ai/search")
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Exa HTTP {}", resp.status());
    }

    #[derive(serde::Deserialize)]
    struct ExaResp {
        #[serde(default)]
        results: Vec<ExaItem>,
    }
    #[derive(serde::Deserialize)]
    struct ExaItem {
        #[serde(default)]
        title: String,
        url: String,
        #[serde(default)]
        score: f64,
    }

    let data: ExaResp = resp.json().await?;
    let results = data
        .results
        .into_iter()
        .enumerate()
        .map(|(i, r)| SearchResult {
            title: r.title,
            url: r.url,
            snippet: String::new(),
            source: "exa".to_string(),
            rank: i,
            score: r.score,
        })
        .collect();

    Ok(("exa".to_string(), results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_normalization() {
        assert_eq!(normalize_url("https://www.example.com/"), "example.com");
        assert_eq!(normalize_url("http://Example.COM/path"), "example.com/path");
        assert_eq!(normalize_url("https://example.com"), "example.com");
    }

    #[test]
    fn merge_deduplication() {
        let results = vec![
            SearchResult {
                title: "Test".into(),
                url: "https://example.com".into(),
                snippet: "test snippet".into(),
                source: "ddg".into(),
                rank: 0,
                score: 0.0,
            },
            SearchResult {
                title: "Test 2".into(),
                url: "https://www.example.com/".into(),
                snippet: "same site".into(),
                source: "brave".into(),
                rank: 0,
                score: 0.0,
            },
            SearchResult {
                title: "Different".into(),
                url: "https://other.com".into(),
                snippet: "other".into(),
                source: "ddg".into(),
                rank: 1,
                score: 0.0,
            },
        ];
        let merged = merge_and_rank(results, 10);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn ddg_url_decode() {
        let raw = "/l/?uddg=https%3A%2F%2Fexample.com&rut=abc";
        assert_eq!(decode_ddg_url(raw), "https://example.com");
    }

    #[test]
    fn strip_html_tags() {
        assert_eq!(strip_tags("<b>hello</b> world"), "hello world");
        assert_eq!(strip_tags("a &amp; b"), "a & b");
    }

    #[tokio::test]
    async fn empty_query_rejected() {
        let tool = MultiSearchTool::new(5, 10, None, None);
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(!result.success);
    }
}
