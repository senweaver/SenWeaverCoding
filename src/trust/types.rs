// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrustConfig {

    #[serde(default = "default_initial_score")]
    pub initial_score: f64,

    #[serde(default = "default_decay_half_life")]
    pub decay_half_life_days: f64,

    #[serde(default = "default_regression_threshold")]
    pub regression_threshold: f64,

    #[serde(default = "default_correction_penalty")]
    pub correction_penalty: f64,

    #[serde(default = "default_success_boost")]
    pub success_boost: f64,
}

fn default_initial_score() -> f64 {
    0.8
}
fn default_decay_half_life() -> f64 {
    30.0
}
fn default_regression_threshold() -> f64 {
    0.5
}
fn default_correction_penalty() -> f64 {
    0.05
}
fn default_success_boost() -> f64 {
    0.01
}

impl Default for TrustConfig {
    fn default() -> Self {
        Self {
            initial_score: default_initial_score(),
            decay_half_life_days: default_decay_half_life(),
            regression_threshold: default_regression_threshold(),
            correction_penalty: default_correction_penalty(),
            success_boost: default_success_boost(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    pub domain: String,
    pub score: f64,
    pub last_updated: DateTime<Utc>,
    pub event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionType {
    UserOverride,
    QualityFailure,
    SopDeviation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionEvent {
    pub domain: String,
    pub correction_type: CorrectionType,
    pub description: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionAlert {
    pub domain: String,
    pub current_score: f64,
    pub threshold: f64,
    pub detected_at: DateTime<Utc>,
}

pub struct TrustTracker {
    config: TrustConfig,
    scores: HashMap<String, TrustScore>,
    correction_log: Vec<CorrectionEvent>,
    store: Option<crate::trust::store::TrustStore>,
}

impl TrustTracker {
    pub fn new(config: TrustConfig) -> Self {
        Self {
            config,
            scores: HashMap::new(),
            correction_log: Vec::new(),
            store: None,
        }
    }

    pub fn new_persistent(config: TrustConfig, dir: &std::path::Path) -> Self {
        let store = crate::trust::store::TrustStore::new(dir);
        let scores = store.load();
        if !scores.is_empty() {
            tracing::info!(domains = scores.len(), "Loaded persisted trust scores");
        }
        Self {
            config,
            scores,
            correction_log: Vec::new(),
            store: Some(store),
        }
    }

    fn flush(&self) {
        if let Some(ref store) = self.store {
            if let Err(e) = store.save(&self.scores) {
                tracing::warn!("Failed to persist trust scores: {e}");
            }
        }
    }

    pub fn get_score(&mut self, domain: &str) -> f64 {
        self.ensure_domain(domain);
        self.scores[domain].score
    }

    pub fn record_correction(
        &mut self,
        domain: &str,
        correction_type: CorrectionType,
        description: &str,
    ) {
        self.ensure_domain(domain);
        let now = Utc::now();

        let Some(score) = self.scores.get_mut(domain) else {
            return;
        };
        score.score = (score.score - self.config.correction_penalty).max(0.0);
        score.last_updated = now;
        score.event_count += 1;

        self.correction_log.push(CorrectionEvent {
            domain: domain.to_string(),
            correction_type,
            description: description.to_string(),
            timestamp: now,
        });

        self.flush();
    }

    pub fn record_success(&mut self, domain: &str) {
        self.ensure_domain(domain);
        let now = Utc::now();

        let Some(score) = self.scores.get_mut(domain) else {
            return;
        };
        score.score = (score.score + self.config.success_boost).min(1.0);
        score.last_updated = now;
        score.event_count += 1;

        self.flush();
    }

    pub fn apply_decay(&mut self, now: DateTime<Utc>) {
        let half_life_secs = self.config.decay_half_life_days * 86400.0;

        for score in self.scores.values_mut() {
            let elapsed_secs = (now - score.last_updated).num_seconds() as f64;
            if elapsed_secs <= 0.0 {
                continue;
            }

            let decay_factor = 0.5_f64.powf(elapsed_secs / half_life_secs);
            let initial = self.config.initial_score;

            score.score = initial + (score.score - initial) * decay_factor;
            score.last_updated = now;
        }
    }

    pub fn check_regression(&mut self, domain: &str) -> Option<RegressionAlert> {
        self.ensure_domain(domain);
        let score = &self.scores[domain];
        if score.score < self.config.regression_threshold {
            Some(RegressionAlert {
                domain: domain.to_string(),
                current_score: score.score,
                threshold: self.config.regression_threshold,
                detected_at: Utc::now(),
            })
        } else {
            None
        }
    }

    pub fn corrections_for_domain(&self, domain: &str) -> Vec<&CorrectionEvent> {
        self.correction_log
            .iter()
            .filter(|e| e.domain == domain)
            .collect()
    }

    pub fn domains(&self) -> Vec<&str> {
        self.scores.keys().map(|s| s.as_str()).collect()
    }

    pub fn correction_log(&self) -> &[CorrectionEvent] {
        &self.correction_log
    }

    pub fn snapshot(&self) -> HashMap<String, TrustScore> {
        self.scores.clone()
    }

    pub fn config(&self) -> &TrustConfig {
        &self.config
    }

    fn ensure_domain(&mut self, domain: &str) {
        if !self.scores.contains_key(domain) {
            self.scores.insert(
                domain.to_string(),
                TrustScore {
                    domain: domain.to_string(),
                    score: self.config.initial_score,
                    last_updated: Utc::now(),
                    event_count: 0,
                },
            );
        }
    }
}
