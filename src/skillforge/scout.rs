// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoutSource {
    GitHub,
    ClawHub,
    HuggingFace,
}

impl std::str::FromStr for ScoutSource {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "github" => Self::GitHub,
            "clawhub" => Self::ClawHub,
            "huggingface" | "hf" => Self::HuggingFace,
            _ => {
                warn!(source = s, "Unknown scout source, defaulting to GitHub");
                Self::GitHub
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoutResult {
    pub name: String,
    pub url: String,
    pub description: String,
    pub stars: u64,
    pub language: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub source: ScoutSource,

    pub owner: String,

    pub has_license: bool,
}

#[async_trait]
pub trait Scout: Send + Sync {

    async fn discover(&self) -> Result<Vec<ScoutResult>>;
}

pub struct GitHubScout {
    client: reqwest::Client,
    queries: Vec<String>,
}

impl GitHubScout {
    pub fn new(token: Option<String>) -> Self {
        use std::time::Duration;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            "application/vnd.github+json".parse().expect("valid header"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            "SenWeaverCoding-SkillForge/0.1"
                .parse()
                .expect("valid header"),
        );
        if let Some(ref t) = token {
            if let Ok(val) = format!("Bearer {t}").parse() {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            queries: vec!["sen skill".into(), "ai agent skill".into()],
        }
    }

    fn parse_items(body: &serde_json::Value) -> Vec<ScoutResult> {
        let items = match body.get("items").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return vec![],
        };

        items
            .iter()
            .filter_map(|item| {
                let name = item.get("name")?.as_str()?.to_string();
                let url = item.get("html_url")?.as_str()?.to_string();
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let stars = item
                    .get("stargazers_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let language = item
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let updated_at = item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                let owner = item
                    .get("owner")
                    .and_then(|o| o.get("login"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let has_license = item.get("license").map(|v| !v.is_null()).unwrap_or(false);

                Some(ScoutResult {
                    name,
                    url,
                    description,
                    stars,
                    language,
                    updated_at,
                    source: ScoutSource::GitHub,
                    owner,
                    has_license,
                })
            })
            .collect()
    }
}

#[async_trait]
impl Scout for GitHubScout {
    async fn discover(&self) -> Result<Vec<ScoutResult>> {
        let mut all: Vec<ScoutResult> = Vec::new();

        for query in &self.queries {
            let url = format!(
                "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page=30",
                urlencoding(query)
            );
            debug!(query = query.as_str(), "Searching GitHub");

            let resp = match self.client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        query = query.as_str(),
                        error = %e,
                        "GitHub API request failed, skipping query"
                    );
                    continue;
                }
            };

            if !resp.status().is_success() {
                warn!(
                    status = %resp.status(),
                    query = query.as_str(),
                    "GitHub search returned non-200"
                );
                continue;
            }

            let body: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        query = query.as_str(),
                        error = %e,
                        "Failed to parse GitHub response, skipping query"
                    );
                    continue;
                }
            };

            let mut items = Self::parse_items(&body);
            debug!(count = items.len(), query = query.as_str(), "Parsed items");
            all.append(&mut items);
        }

        dedup(&mut all);
        Ok(all)
    }
}

fn urlencoding(s: &str) -> String {
    s.replace(' ', "+").replace('&', "%26").replace('#', "%23")
}

pub fn dedup(results: &mut Vec<ScoutResult>) {
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| seen.insert(r.url.clone()));
}

