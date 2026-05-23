// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

use schemars::JsonSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityTier {

    Simple,

    Standard,

    Complex,
}

const REASONING_KEYWORDS: &[&str] = &[
    "explain",
    "why",
    "analyze",
    "compare",
    "design",
    "implement",
    "refactor",
    "debug",
    "optimize",
    "architecture",
    "trade-off",
    "tradeoff",
    "reasoning",
    "step by step",
    "think through",
    "evaluate",
    "critique",
    "pros and cons",
];

pub fn estimate_complexity(message: &str) -> ComplexityTier {
    let lower = message.to_lowercase();
    let len = message.len();

    let keyword_count = REASONING_KEYWORDS
        .iter()
        .filter(|kw| lower.contains(**kw))
        .count();

    let has_code_fence = message.contains("```");

    if len > 200 || has_code_fence || keyword_count >= 2 {
        return ComplexityTier::Complex;
    }

    if len < 50 && keyword_count == 0 {
        return ComplexityTier::Simple;
    }

    ComplexityTier::Standard
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AutoClassifyConfig {

    #[serde(default)]
    pub simple_hint: Option<String>,

    #[serde(default)]
    pub standard_hint: Option<String>,

    #[serde(default)]
    pub complex_hint: Option<String>,

    #[serde(default = "default_cost_optimized_hint")]
    pub cost_optimized_hint: String,
}

fn default_cost_optimized_hint() -> String {
    "cost-optimized".to_string()
}

impl Default for AutoClassifyConfig {
    fn default() -> Self {
        Self {
            simple_hint: None,
            standard_hint: None,
            complex_hint: None,
            cost_optimized_hint: default_cost_optimized_hint(),
        }
    }
}

impl AutoClassifyConfig {

    pub fn hint_for(&self, tier: ComplexityTier) -> Option<&str> {
        match tier {
            ComplexityTier::Simple => self.simple_hint.as_deref(),
            ComplexityTier::Standard => self.standard_hint.as_deref(),
            ComplexityTier::Complex => self.complex_hint.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvalConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_min_quality_score")]
    pub min_quality_score: f64,

    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_min_quality_score() -> f64 {
    0.5
}

fn default_max_retries() -> u32 {
    1
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_quality_score: default_min_quality_score(),
            max_retries: default_max_retries(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvalResult {

    pub score: f64,

    pub checks: Vec<EvalCheck>,

    pub retry_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EvalCheck {
    pub name: &'static str,
    pub passed: bool,
    pub weight: f64,
}

const CODE_KEYWORDS: &[&str] = &[
    "code",
    "function",
    "implement",
    "class",
    "struct",
    "module",
    "script",
    "program",
    "bug",
    "error",
    "compile",
    "syntax",
    "refactor",
];

pub fn evaluate_response(
    query: &str,
    response: &str,
    complexity: ComplexityTier,
    auto_classify: Option<&AutoClassifyConfig>,
) -> EvalResult {
    let mut checks = Vec::new();

    let non_empty = !response.trim().is_empty();
    checks.push(EvalCheck {
        name: "non_empty",
        passed: non_empty,
        weight: 0.3,
    });

    let lower_resp = response.to_lowercase();
    let cop_out_phrases = [
        "i don't know",
        "i'm not sure",
        "i cannot",
        "i can't help",
        "as an ai",
    ];
    let is_cop_out = cop_out_phrases
        .iter()
        .any(|phrase| lower_resp.starts_with(phrase));
    let not_cop_out = !is_cop_out || response.len() > 200;
    checks.push(EvalCheck {
        name: "not_cop_out",
        passed: not_cop_out,
        weight: 0.25,
    });

    let min_len = match complexity {
        ComplexityTier::Simple => 5,
        ComplexityTier::Standard => 20,
        ComplexityTier::Complex => 50,
    };
    let sufficient_length = response.len() >= min_len;
    checks.push(EvalCheck {
        name: "sufficient_length",
        passed: sufficient_length,
        weight: 0.2,
    });

    let query_lower = query.to_lowercase();
    let expects_code = CODE_KEYWORDS.iter().any(|kw| query_lower.contains(kw));
    let has_code = response.contains("```") || response.contains("    ");
    let code_check_passed = !expects_code || has_code;
    checks.push(EvalCheck {
        name: "code_presence",
        passed: code_check_passed,
        weight: 0.25,
    });

    let total_weight: f64 = checks.iter().map(|c| c.weight).sum();
    let earned: f64 = checks.iter().filter(|c| c.passed).map(|c| c.weight).sum();
    let score = if total_weight > 0.0 {
        earned / total_weight
    } else {
        1.0
    };

    let retry_hint = if score <= default_min_quality_score() {

        let next_tier = match complexity {
            ComplexityTier::Simple => Some(ComplexityTier::Standard),
            ComplexityTier::Standard => Some(ComplexityTier::Complex),
            ComplexityTier::Complex => None,
        };
        next_tier.and_then(|tier| {
            auto_classify
                .and_then(|ac| ac.hint_for(tier))
                .map(String::from)
        })
    } else {
        None
    };

    EvalResult {
        score,
        checks,
        retry_hint,
    }
}
