// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

use super::scout::ScoutResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scores {

    pub compatibility: f64,

    pub quality: f64,

    pub security: f64,
}

impl Scores {

    pub fn total(&self) -> f64 {
        self.compatibility * 0.30 + self.quality * 0.35 + self.security * 0.35
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recommendation {

    Auto,

    Manual,

    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub candidate: ScoutResult,
    pub scores: Scores,
    pub total_score: f64,
    pub recommendation: Recommendation,
}

pub struct Evaluator {

    min_score: f64,
}

const BAD_PATTERNS: &[&str] = &[
    "malware",
    "exploit",
    "hack",
    "crack",
    "keygen",
    "ransomware",
    "trojan",
];

fn contains_word(haystack: &str, word: &str) -> bool {
    for (i, _) in haystack.match_indices(word) {
        let before_ok = i == 0 || !haystack.as_bytes()[i - 1].is_ascii_alphanumeric();
        let after = i + word.len();
        let after_ok =
            after >= haystack.len() || !haystack.as_bytes()[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

impl Evaluator {
    pub fn new(min_score: f64) -> Self {
        Self { min_score }
    }

    pub fn evaluate(&self, candidate: ScoutResult) -> EvalResult {
        let compatibility = self.score_compatibility(&candidate);
        let quality = self.score_quality(&candidate);
        let security = self.score_security(&candidate);

        let scores = Scores {
            compatibility,
            quality,
            security,
        };
        let total_score = scores.total();

        let recommendation = if total_score >= self.min_score {
            Recommendation::Auto
        } else if total_score >= 0.4 {
            Recommendation::Manual
        } else {
            Recommendation::Skip
        };

        EvalResult {
            candidate,
            scores,
            total_score,
            recommendation,
        }
    }

    fn score_compatibility(&self, c: &ScoutResult) -> f64 {
        match c.language.as_deref() {
            Some("Rust") => 1.0,
            Some("Python" | "TypeScript" | "JavaScript") => 0.6,
            Some(_) => 0.3,
            None => 0.2,
        }
    }

    fn score_quality(&self, c: &ScoutResult) -> f64 {

        let raw = ((c.stars as f64) + 1.0).log2() / 10.0;
        raw.min(1.0)
    }

    fn score_security(&self, c: &ScoutResult) -> f64 {
        let mut score: f64 = 0.5;

        if c.has_license {
            score += 0.3;
        }

        let lower_name = c.name.to_lowercase();
        let lower_desc = c.description.to_lowercase();
        for pat in BAD_PATTERNS {
            if contains_word(&lower_name, pat) || contains_word(&lower_desc, pat) {
                score -= 0.5;
                break;
            }
        }

        if let Some(updated) = c.updated_at {
            let age_days = (chrono::Utc::now() - updated).num_days();
            if (0..180).contains(&age_days) {
                score += 0.2;
            }
        }

        score.clamp(0.0, 1.0)
    }
}

