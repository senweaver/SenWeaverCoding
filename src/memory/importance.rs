// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Heuristic importance scorer for non-LLM paths.
//!
//! Assigns importance scores (0.0–1.0) based on memory category and keyword
//! signals. Used when LLM-based consolidation is unavailable or as a fast
//! first-pass scorer.

use super::traits::MemoryCategory;

fn category_base_score(category: &MemoryCategory) -> f64 {
    match category {
        MemoryCategory::Core => 0.7,
        MemoryCategory::Daily => 0.3,
        MemoryCategory::Conversation => 0.2,
        MemoryCategory::Custom(_) => 0.4,
    }
}

fn keyword_boost(content: &str) -> f64 {
    const HIGH_SIGNAL_KEYWORDS: &[&str] = &[
        "decision",
        "always",
        "never",
        "important",
        "critical",
        "must",
        "requirement",
        "policy",
        "rule",
        "principle",
    ];

    let lowered = content.to_ascii_lowercase();
    let matches = HIGH_SIGNAL_KEYWORDS
        .iter()
        .filter(|kw| lowered.contains(**kw))
        .count();

    (matches as f64 * 0.1).min(0.2)
}

pub fn compute_importance(content: &str, category: &MemoryCategory) -> f64 {
    let base = category_base_score(category);
    let boost = keyword_boost(content);
    (base + boost).min(1.0)
}

pub fn weighted_final_score(hybrid_score: f64, importance: f64, recency_decay: f64) -> f64 {
    hybrid_score * 0.7 + importance * 0.2 + recency_decay * 0.1
}
