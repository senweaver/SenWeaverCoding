// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShellToolConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub allowed_commands: Vec<String>,

    #[serde(default)]
    pub working_dir: Option<String>,

    #[serde(default = "default_shell_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for ShellToolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_commands: Vec::new(),
            working_dir: None,
            timeout_secs: default_shell_timeout_secs(),
        }
    }
}

fn default_shell_timeout_secs() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebSearchConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_search_provider")]
    pub default_provider: String,

    #[serde(default)]
    pub serpapi_key: Option<String>,

    #[serde(default)]
    pub serper_api_key: Option<String>,

    #[serde(default)]
    pub tavily_api_key: Option<String>,

    #[serde(default)]
    pub exa_api_key: Option<String>,

    #[serde(default)]
    pub google_api_key: Option<String>,

    #[serde(default)]
    pub google_cx: Option<String>,

    #[serde(default = "default_search_results")]
    pub num_results: u8,

    #[serde(default)]
    pub region: String,

    #[serde(default)]
    pub language: String,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_provider: default_search_provider(),
            serpapi_key: None,
            serper_api_key: None,
            tavily_api_key: None,
            exa_api_key: None,
            google_api_key: None,
            google_cx: None,
            num_results: default_search_results(),
            region: String::new(),
            language: String::new(),
        }
    }
}

fn default_search_provider() -> String {
    "duckduckgo".into()
}

fn default_search_results() -> u8 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpRequestConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub allowed_domains: Vec<String>,

    #[serde(default = "default_http_max_response_size")]
    pub max_response_size: usize,

    #[serde(default = "default_http_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub allow_private_hosts: bool,
}

impl Default for HttpRequestConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_domains: vec!["*".into()],
            max_response_size: default_http_max_response_size(),
            timeout_secs: default_http_timeout_secs(),
            allow_private_hosts: false,
        }
    }
}

fn default_http_max_response_size() -> usize {
    1_000_000
}

fn default_http_timeout_secs() -> u64 {
    30
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FirecrawlMode {
    #[default]
    Scrape,

    Crawl,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FirecrawlConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_firecrawl_api_key_env")]
    pub api_key_env: String,

    #[serde(default = "default_firecrawl_api_url")]
    pub api_url: String,

    #[serde(default)]
    pub mode: FirecrawlMode,
}

fn default_firecrawl_api_key_env() -> String {
    "FIRECRAWL_API_KEY".into()
}

fn default_firecrawl_api_url() -> String {
    "https://api.firecrawl.dev/v1".into()
}

impl Default for FirecrawlConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key_env: default_firecrawl_api_key_env(),
            api_url: default_firecrawl_api_url(),
            mode: FirecrawlMode::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebFetchConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_web_fetch_allowed_domains")]
    pub allowed_domains: Vec<String>,

    #[serde(default)]
    pub blocked_domains: Vec<String>,

    #[serde(default)]
    pub allowed_private_hosts: Vec<String>,

    #[serde(default = "default_web_fetch_max_response_size")]
    pub max_response_size: usize,

    #[serde(default = "default_web_fetch_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub firecrawl: FirecrawlConfig,
}

fn default_web_fetch_max_response_size() -> usize {
    500_000
}

fn default_web_fetch_timeout_secs() -> u64 {
    30
}

fn default_web_fetch_allowed_domains() -> Vec<String> {
    vec!["*".into()]
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_domains: vec!["*".into()],
            blocked_domains: vec![],
            allowed_private_hosts: vec![],
            max_response_size: default_web_fetch_max_response_size(),
            timeout_secs: default_web_fetch_timeout_secs(),
            firecrawl: FirecrawlConfig::default(),
        }
    }
}

impl WebFetchConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.max_response_size == 0 {
            errors.push(
                "web_fetch.max_response_size = 0 disables size limits (may cause memory issues)"
                    .into(),
            );
        }
        if self.timeout_secs == 0 {
            errors.push("web_fetch.timeout_secs = 0 is not allowed".into());
        }
        for domain in &self.allowed_domains {
            if domain.is_empty() {
                errors.push("web_fetch.allowed_domains contains empty string".into());
            }
        }
        errors
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextBrowserConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_text_browser_viewport_width")]
    pub viewport_width: usize,

    #[serde(default = "default_text_browser_scroll_step")]
    pub scroll_step: usize,

    #[serde(default)]
    pub download_dir: Option<String>,

    #[serde(default = "default_text_browser_user_agent")]
    pub user_agent: String,

    #[serde(default)]
    pub accept_invalid_certs: bool,
}

impl Default for TextBrowserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            viewport_width: default_text_browser_viewport_width(),
            scroll_step: default_text_browser_scroll_step(),
            download_dir: None,
            user_agent: default_text_browser_user_agent(),
            accept_invalid_certs: false,
        }
    }
}

fn default_text_browser_viewport_width() -> usize {
    80
}

fn default_text_browser_scroll_step() -> usize {
    50
}

fn default_text_browser_user_agent() -> String {
    "Mozilla/5.0 (compatible; SenWeaverCoding/1.0)".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinkEnricherConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub services: Vec<String>,

    #[serde(default = "default_link_enricher_max_links")]
    pub max_links: usize,

    #[serde(default = "default_link_enricher_timeout")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub include_content_preview: bool,

    #[serde(default = "default_link_enricher_preview_length")]
    pub content_preview_length: usize,
}

impl Default for LinkEnricherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            services: vec!["opengraph".into(), "twitter_card".into()],
            max_links: default_link_enricher_max_links(),
            timeout_secs: default_link_enricher_timeout(),
            include_content_preview: false,
            content_preview_length: default_link_enricher_preview_length(),
        }
    }
}

fn default_link_enricher_max_links() -> usize {
    10
}

fn default_link_enricher_timeout() -> u64 {
    5
}

fn default_link_enricher_preview_length() -> usize {
    500
}
