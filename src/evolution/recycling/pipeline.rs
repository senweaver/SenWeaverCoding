// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;
use std::collections::HashSet;

use super::types::{RecycledExperience, RecycledExperienceOutcome};
use crate::config::domain::evolution::ExperienceRecyclingConfig;

#[derive(Debug, Clone)]
pub struct ExperienceRank {
    pub experience: RecycledExperience,
    pub score: f32,
    pub quality: f32,
    pub recency: f32,
    pub diversity: f32,
    pub relevance: f32,
}

const WEIGHT_RELEVANCE: f32 = 0.35;

pub fn rank_experiences(
    experiences: Vec<RecycledExperience>,
    coding_mode: Option<&str>,
    user_message: Option<&str>,
    config: &ExperienceRecyclingConfig,
) -> Vec<ExperienceRank> {
    if experiences.is_empty() {
        return Vec::new();
    }
    let now_ms = Utc::now().timestamp_millis();
    let recency_span_ms: i64 = 14 * 24 * 60 * 60 * 1000;
    let weight_quality = config.weight_quality.max(0.0);
    let weight_recency = config.weight_recency.max(0.0);
    let weight_diversity = config.weight_diversity.max(0.0);
    let query_tokens = user_message.map(relevance_tokens).unwrap_or_default();
    let weight_relevance = if query_tokens.is_empty() {
        0.0
    } else {
        WEIGHT_RELEVANCE
    };
    let total_weight = (weight_quality + weight_recency + weight_diversity + weight_relevance)
        .max(f32::EPSILON);
    let mut scored: Vec<ExperienceRank> = experiences
        .into_iter()
        .map(|exp| {
            let quality = quality_score(&exp);
            let recency = recency_score(now_ms, exp.created_at.timestamp_millis(), recency_span_ms);
            let relevance = if query_tokens.is_empty() {
                0.0
            } else {
                relevance_score(&query_tokens, &exp)
            };
            ExperienceRank {
                experience: exp,
                score: 0.0,
                quality,
                recency,
                diversity: 1.0,
                relevance,
            }
        })
        .collect();
    if let Some(active) = coding_mode {
        let active_lower = active.to_ascii_lowercase();
        for entry in &mut scored {
            if entry
                .experience
                .coding_mode
                .as_deref()
                .map(|m| m.to_ascii_lowercase() == active_lower)
                .unwrap_or(false)
            {
                entry.quality = (entry.quality + 0.1).clamp(0.0, 1.0);
            }
        }
    }
    scored.sort_by(|a, b| {
        b.experience
            .created_at
            .cmp(&a.experience.created_at)
    });
    let mut seen_signatures: HashSet<String> = HashSet::new();
    for entry in &mut scored {
        if seen_signatures.contains(&entry.experience.shape_signature) {
            entry.diversity = 0.2;
        } else {
            seen_signatures.insert(entry.experience.shape_signature.clone());
            entry.diversity = 1.0;
        }
        let acc = entry.quality * weight_quality
            + entry.recency * weight_recency
            + entry.diversity * weight_diversity
            + entry.relevance * weight_relevance;
        entry.score = acc / total_weight;
    }
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

fn relevance_tokens(text: &str) -> HashSet<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "that", "this", "from", "have", "will", "your", "please",
        "into", "what", "when", "where", "how", "can", "could", "should", "would", "about",
    ];
    let mut out: HashSet<String> = HashSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                current.push(lower);
            }
        } else if !current.is_empty() {
            crate::evolution::text_match::push_segment_tokens(
                &std::mem::take(&mut current),
                3,
                STOPWORDS,
                &mut out,
            );
        }
    }
    if !current.is_empty() {
        crate::evolution::text_match::push_segment_tokens(&current, 3, STOPWORDS, &mut out);
    }
    out
}

fn relevance_score(query_tokens: &HashSet<String>, exp: &RecycledExperience) -> f32 {
    let mut doc = String::with_capacity(
        exp.headline.len() + exp.context_excerpt.len() + exp.tools_summary.len() + 32,
    );
    doc.push_str(&exp.headline);
    doc.push(' ');
    doc.push_str(&exp.context_excerpt);
    doc.push(' ');
    doc.push_str(&exp.tools_summary);
    for tag in &exp.tags {
        doc.push(' ');
        doc.push_str(tag);
    }
    let doc_tokens = relevance_tokens(&doc);
    if doc_tokens.is_empty() {
        return 0.0;
    }
    let overlap = query_tokens.intersection(&doc_tokens).count();
    let denom = query_tokens.len().min(doc_tokens.len()).max(1);
    #[allow(clippy::cast_precision_loss)]
    let score = overlap as f32 / denom as f32;
    score.clamp(0.0, 1.0)
}

fn quality_score(exp: &RecycledExperience) -> f32 {
    let mut base = (exp.reward + 1.0) * 0.5;
    base = base.clamp(0.0, 1.0);
    let outcome_bonus = match exp.outcome {
        RecycledExperienceOutcome::Success => 0.10,
        RecycledExperienceOutcome::Failure => 0.0,
        RecycledExperienceOutcome::Neutral => 0.05,
    };
    (base + outcome_bonus).clamp(0.0, 1.0)
}

fn recency_score(now_ms: i64, created_ms: i64, span_ms: i64) -> f32 {
    let span = span_ms.max(1);
    let age = (now_ms - created_ms).max(0);
    if age >= span {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let ratio = 1.0 - (age as f32 / span as f32);
    ratio.clamp(0.0, 1.0)
}
