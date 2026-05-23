// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod evaluate;
pub mod integrate;
pub mod scout;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use self::evaluate::{EvalResult, Evaluator, Recommendation};
use self::integrate::Integrator;
use self::scout::{GitHubScout, Scout, ScoutResult, ScoutSource};

#[derive(Clone, Serialize, Deserialize)]
pub struct SkillForgeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_auto_integrate")]
    pub auto_integrate: bool,
    #[serde(default = "default_sources")]
    pub sources: Vec<String>,
    #[serde(default = "default_scan_interval")]
    pub scan_interval_hours: u64,
    #[serde(default = "default_min_score")]
    pub min_score: f64,

    #[serde(default)]
    pub github_token: Option<String>,

    #[serde(default = "default_output_dir")]
    pub output_dir: String,
}

fn default_auto_integrate() -> bool {
    true
}
fn default_sources() -> Vec<String> {
    vec!["github".into(), "clawhub".into()]
}
fn default_scan_interval() -> u64 {
    24
}
fn default_min_score() -> f64 {
    0.7
}
fn default_output_dir() -> String {
    "./skills".into()
}

impl Default for SkillForgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_integrate: default_auto_integrate(),
            sources: default_sources(),
            scan_interval_hours: default_scan_interval(),
            min_score: default_min_score(),
            github_token: None,
            output_dir: default_output_dir(),
        }
    }
}

impl std::fmt::Debug for SkillForgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillForgeConfig")
            .field("enabled", &self.enabled)
            .field("auto_integrate", &self.auto_integrate)
            .field("sources", &self.sources)
            .field("scan_interval_hours", &self.scan_interval_hours)
            .field("min_score", &self.min_score)
            .field("github_token", &self.github_token.as_ref().map(|_| "***"))
            .field("output_dir", &self.output_dir)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeReport {
    pub discovered: usize,
    pub evaluated: usize,
    pub auto_integrated: usize,
    pub manual_review: usize,
    pub skipped: usize,
    pub results: Vec<EvalResult>,
}

pub struct SkillForge {
    config: SkillForgeConfig,
    evaluator: Evaluator,
    integrator: Integrator,
}

impl SkillForge {
    pub fn new(config: SkillForgeConfig) -> Self {
        let evaluator = Evaluator::new(config.min_score);
        let integrator = Integrator::new(config.output_dir.clone());
        Self {
            config,
            evaluator,
            integrator,
        }
    }

    pub async fn forge(&self) -> Result<ForgeReport> {
        if !self.config.enabled {
            warn!("SkillForge is disabled — skipping");
            return Ok(ForgeReport {
                discovered: 0,
                evaluated: 0,
                auto_integrated: 0,
                manual_review: 0,
                skipped: 0,
                results: vec![],
            });
        }

        let mut candidates: Vec<ScoutResult> = Vec::new();

        for src in &self.config.sources {
            let source = match src.parse() {
                Ok(s) => s,
                Err(e) => {
                    warn!(source = %src, error = %e, "SkillForge: skipping invalid source");
                    continue;
                }
            };
            match source {
                ScoutSource::GitHub => {
                    let scout = GitHubScout::new(self.config.github_token.clone());
                    match scout.discover().await {
                        Ok(mut found) => {
                            info!(count = found.len(), "GitHub scout returned candidates");
                            candidates.append(&mut found);
                        }
                        Err(e) => {
                            warn!(error = %e, "GitHub scout failed, continuing with other sources");
                        }
                    }
                }
                ScoutSource::ClawHub => {
                    let base_url = std::env::var("CLAWHUB_API_URL")
                        .unwrap_or_else(|_| "https://api.clawhub.dev/v1".to_string());
                    let url = format!("{base_url}/skills?query=sen&limit=20");
                    match reqwest::Client::new()
                        .get(&url)
                        .header("Accept", "application/json")
                        .timeout(std::time::Duration::from_secs(10))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(body) = resp.json::<serde_json::Value>().await {
                                if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
                                    let found: Vec<ScoutResult> = items
                                        .iter()
                                        .filter_map(|item| {
                                            Some(ScoutResult {
                                                name: item.get("name")?.as_str()?.to_string(),
                                                url: item.get("url")?.as_str()?.to_string(),
                                                description: item
                                                    .get("description")
                                                    .and_then(|d| d.as_str())
                                                    .unwrap_or("")
                                                    .to_string(),
                                                stars: item
                                                    .get("stars")
                                                    .and_then(|s| s.as_u64())
                                                    .unwrap_or(0),
                                                language: item
                                                    .get("language")
                                                    .and_then(|l| l.as_str())
                                                    .map(String::from),
                                                updated_at: item
                                                    .get("updated_at")
                                                    .and_then(|v| v.as_str())
                                                    .and_then(|s| {
                                                        s.parse::<chrono::DateTime<chrono::Utc>>()
                                                            .ok()
                                                    }),
                                                source: ScoutSource::ClawHub,
                                                owner: item
                                                    .get("owner")
                                                    .and_then(|o| o.as_str())
                                                    .unwrap_or("unknown")
                                                    .to_string(),
                                                has_license: item
                                                    .get("license")
                                                    .map(|v| !v.is_null())
                                                    .unwrap_or(false),
                                            })
                                        })
                                        .collect();
                                    info!(count = found.len(), "ClawHub scout returned candidates");
                                    candidates.extend(found);
                                }
                            }
                        }
                        Ok(resp) => {
                            warn!(status = %resp.status(), "ClawHub API returned non-success");
                        }
                        Err(e) => {
                            warn!(error = %e, "ClawHub scout failed, continuing");
                        }
                    }
                }
                ScoutSource::HuggingFace => {
                    let url = "https://huggingface.co/api/models?search=sen-skill&limit=20";
                    match reqwest::Client::new()
                        .get(url)
                        .header("Accept", "application/json")
                        .timeout(std::time::Duration::from_secs(10))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(items) = resp.json::<Vec<serde_json::Value>>().await {
                                let found: Vec<ScoutResult> = items
                                    .iter()
                                    .filter_map(|item| {
                                        let model_id = item.get("modelId")?.as_str()?.to_string();
                                        Some(ScoutResult {
                                            name: model_id.clone(),
                                            url: format!("https://huggingface.co/{model_id}"),
                                            description: item
                                                .get("pipeline_tag")
                                                .and_then(|d| d.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            stars: item
                                                .get("likes")
                                                .and_then(|s| s.as_u64())
                                                .unwrap_or(0),
                                            language: None,
                                            updated_at: item
                                                .get("lastModified")
                                                .and_then(|v| v.as_str())
                                                .and_then(|s| {
                                                    s.parse::<chrono::DateTime<chrono::Utc>>().ok()
                                                }),
                                            source: ScoutSource::HuggingFace,
                                            owner: model_id
                                                .split('/')
                                                .next()
                                                .unwrap_or("unknown")
                                                .to_string(),
                                            has_license: item
                                                .get("cardData")
                                                .and_then(|c| c.get("license"))
                                                .map(|v| !v.is_null())
                                                .unwrap_or(false),
                                        })
                                    })
                                    .collect();
                                info!(count = found.len(), "HuggingFace scout returned candidates");
                                candidates.extend(found);
                            }
                        }
                        Ok(resp) => {
                            warn!(
                                status = %resp.status(),
                                "HuggingFace API returned non-success"
                            );
                        }
                        Err(e) => {
                            warn!(error = %e, "HuggingFace scout failed, continuing");
                        }
                    }
                }
            }
        }

        scout::dedup(&mut candidates);
        let discovered = candidates.len();
        info!(discovered, "Total unique candidates after dedup");

        let results: Vec<EvalResult> = candidates
            .into_iter()
            .map(|c| self.evaluator.evaluate(c))
            .collect();
        let evaluated = results.len();

        let mut auto_integrated = 0usize;
        let mut manual_review = 0usize;
        let mut skipped = 0usize;

        for res in &results {
            match res.recommendation {
                Recommendation::Auto => {
                    if self.config.auto_integrate {
                        match self.integrator.integrate(&res.candidate) {
                            Ok(_) => {
                                auto_integrated += 1;
                            }
                            Err(e) => {
                                warn!(
                                    skill = res.candidate.name.as_str(),
                                    error = %e,
                                    "Integration failed for candidate, continuing"
                                );
                            }
                        }
                    } else {

                        manual_review += 1;
                    }
                }
                Recommendation::Manual => {
                    manual_review += 1;
                }
                Recommendation::Skip => {
                    skipped += 1;
                }
            }
        }

        info!(
            auto_integrated,
            manual_review, skipped, "Forge pipeline complete"
        );

        Ok(ForgeReport {
            discovered,
            evaluated,
            auto_integrated,
            manual_review,
            skipped,
            results,
        })
    }
}

