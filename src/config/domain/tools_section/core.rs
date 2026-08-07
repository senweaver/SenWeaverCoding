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
