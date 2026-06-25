// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchCategory {
    Web,
    Academic,
    Code,
    Cn,
    Social,
    News,
    Video,
    Wiki,
    Lifestyle,
    Forum,
    Image,
}

impl SearchCategory {
    pub fn from_str_loose(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "academic" | "scholar" | "paper" | "papers" | "research" => Self::Academic,
            "code" | "git" | "github" | "repo" | "repos" => Self::Code,
            "cn" | "chinese" | "china" | "zh" | "zh-cn" => Self::Cn,
            "social" | "community" => Self::Social,
            "news" => Self::News,
            "video" | "videos" | "vid" | "movie" | "movies" => Self::Video,
            "wiki" | "wikipedia" | "encyclopedia" | "encyclopaedia" => Self::Wiki,
            "lifestyle" | "tech-life" | "techlife" | "shopping" | "product" | "products" | "ecommerce" => Self::Lifestyle,
            "forum" | "qa" | "q&a" | "stackoverflow" | "hn" | "hackernews" => Self::Forum,
            "image" | "images" | "img" | "photo" | "photos" | "picture" | "pictures" => Self::Image,
            _ => Self::Web,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Academic => "academic",
            Self::Code => "code",
            Self::Cn => "cn",
            Self::Social => "social",
            Self::News => "news",
            Self::Video => "video",
            Self::Wiki => "wiki",
            Self::Lifestyle => "lifestyle",
            Self::Forum => "forum",
            Self::Image => "image",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRange {
    Day,
    Week,
    Month,
    Year,
}

impl TimeRange {
    pub fn from_str_loose(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "d" | "day" | "today" | "24h" => Some(Self::Day),
            "w" | "week" => Some(Self::Week),
            "m" | "month" => Some(Self::Month),
            "y" | "year" => Some(Self::Year),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ApiKeys {
    pub brave: Option<String>,
    pub searxng_url: Option<String>,
    pub tavily: Option<String>,
    pub exa: Option<String>,
    pub serper: Option<String>,
    pub jina: Option<String>,
    pub pubmed_email: Option<String>,
    pub github_token: Option<String>,
    pub semantic_scholar: Option<String>,
    pub core: Option<String>,
    pub hal: Option<String>,
    pub mastodon_instance: Option<String>,
    pub invidious_instance: Option<String>,
    pub gitlab_token: Option<String>,
    pub gitee_token: Option<String>,
    pub youtube_api_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SearchHit {
    pub fn new(engine: impl Into<String>, title: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            description: String::new(),
            source: None,
            engine: engine.into(),
            score: None,
            published_at: None,
            extra: serde_json::Map::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_published_at(mut self, ts: impl Into<String>) -> Self {
        self.published_at = Some(ts.into());
        self
    }

    pub fn with_extra(mut self, key: &str, value: serde_json::Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    pub fn dedup_key(&self) -> String {
        let url = self.url.trim().trim_end_matches('/').to_ascii_lowercase();
        if !url.is_empty() {
            return url;
        }
        format!(
            "{}:{}",
            self.engine.to_ascii_lowercase(),
            self.title.trim().to_ascii_lowercase()
        )
    }

    pub fn academic_dedup_key(&self) -> Option<String> {
        if let Some(d) = self
            .extra
            .get("doi")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().trim_start_matches("https://doi.org/").to_ascii_lowercase())
        {
            if !d.is_empty() {
                return Some(format!("doi:{d}"));
            }
        }
        let lower_url = self.url.trim().to_ascii_lowercase();
        if let Some(idx) = lower_url.find("doi.org/") {
            let tail = &lower_url[idx + "doi.org/".len()..];
            let doi = tail.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
            if !doi.is_empty() {
                return Some(format!("doi:{doi}"));
            }
        }
        if let Some(idx) = lower_url.find("arxiv.org/abs/") {
            let tail = &lower_url[idx + "arxiv.org/abs/".len()..];
            let arxiv_id = tail
                .split(['?', '#', '/'])
                .next()
                .unwrap_or("")
                .trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
            if !arxiv_id.is_empty() {
                return Some(format!("arxiv:{arxiv_id}"));
            }
        }
        if let Some(arxiv_id) = self
            .extra
            .get("arxiv_id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
        {
            if !arxiv_id.is_empty() {
                return Some(format!("arxiv:{arxiv_id}"));
            }
        }
        let normalized = normalize_academic_title(&self.title);
        if normalized.len() >= 6 {
            return Some(format!("title:{normalized}"));
        }
        None
    }
}

fn normalize_academic_title(title: &str) -> String {
    let lower = title.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    for c in lower.chars() {
        if c.is_alphanumeric() || c >= '\u{4e00}' {
            out.push(c);
        } else if c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        }
    }
    out.trim().to_string()
}

#[derive(Debug, Clone)]
pub struct SearchContext {
    pub query: String,
    pub limit: usize,
    pub locale: Option<String>,
    pub time_range: Option<TimeRange>,
    pub safe_search: bool,
    pub category: SearchCategory,
    pub timeout: Duration,
    pub api_keys: ApiKeys,
    pub user_agent: String,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SearchContext {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: 10,
            locale: None,
            time_range: None,
            safe_search: true,
            category: SearchCategory::Web,
            timeout: Duration::from_secs(30),
            api_keys: ApiKeys::default(),
            user_agent: default_user_agent(),
            extra: serde_json::Map::new(),
        }
    }

    pub fn extra_str(&self, key: &str) -> Option<&str> {
        self.extra.get(key).and_then(|v| v.as_str())
    }

    pub fn extra_bool(&self, key: &str) -> Option<bool> {
        self.extra.get(key).and_then(|v| v.as_bool())
    }

    pub fn extra_i64(&self, key: &str) -> Option<i64> {
        self.extra.get(key).and_then(|v| v.as_i64())
    }

    pub fn build_http_client(&self) -> anyhow::Result<reqwest::Client> {
        Ok(crate::services::require_services()
            .proxy_runtime()
            .build_search_client(
                "tool.web_search",
                self.timeout.as_secs().max(1),
                self.user_agent.as_str(),
            ))
    }
}

pub fn default_user_agent() -> String {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/120.0.0.0 Safari/537.36"
        .to_string()
}

pub const ROTATING_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
];

pub fn pick_rotating_user_agent() -> &'static str {
    let idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)) as usize
        % ROTATING_USER_AGENTS.len();
    ROTATING_USER_AGENTS[idx]
}

#[async_trait]
pub trait SearchEngine: Send + Sync {
    fn id(&self) -> &'static str;

    fn label(&self) -> &'static str;

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Web]
    }

    fn requires_api_key(&self) -> bool {
        false
    }

    fn is_available(&self, _keys: &ApiKeys) -> bool {
        true
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>>;
}
