// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Aggregator {
    MajorityVote,
    EmbeddingCluster { similarity_threshold: f32 },
}

#[derive(Debug, Clone)]
pub struct SelfConsistencyResult {
    pub chosen: String,

    pub support: usize,

    pub samples: usize,

    pub agreement: f32,
    pub candidates: Vec<String>,
}

pub fn aggregate(aggregator: &Aggregator, candidates: Vec<String>) -> SelfConsistencyResult {
    match aggregator {
        Aggregator::MajorityVote => majority_vote(candidates),
        Aggregator::EmbeddingCluster {
            similarity_threshold,
        } => embedding_cluster(candidates, *similarity_threshold),
    }
}

fn normalize_for_vote(s: &str) -> String {
    s.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '!', '?', '。', '！', '？'])
        .to_string()
}

fn majority_vote(candidates: Vec<String>) -> SelfConsistencyResult {
    let total = candidates.len();
    if total == 0 {
        return SelfConsistencyResult {
            chosen: String::new(),
            support: 0,
            samples: 0,
            agreement: 0.0,
            candidates,
        };
    }
    let normalized: Vec<String> = candidates.iter().map(|c| normalize_for_vote(c)).collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for n in &normalized {
        *counts.entry(n.as_str()).or_insert(0) += 1;
    }
    let (winner_idx, support) = (0..total)
        .map(|i| (i, counts.get(normalized[i].as_str()).copied().unwrap_or(1)))
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .unwrap_or((0, 1));

    SelfConsistencyResult {
        chosen: candidates[winner_idx].clone(),
        support,
        samples: total,
        agreement: support as f32 / total as f32,
        candidates,
    }
}

fn tokenize(s: &str) -> HashMap<String, f32> {
    let mut freq: HashMap<String, f32> = HashMap::new();
    for w in s.split_whitespace() {
        *freq.entry(w.to_ascii_lowercase()).or_insert(0.0) += 1.0;
    }
    freq
}

fn cosine(a: &HashMap<String, f32>, b: &HashMap<String, f32>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    for (k, va) in a {
        if let Some(vb) = b.get(k) {
            dot += va * vb;
        }
    }
    let norm = |m: &HashMap<String, f32>| m.values().map(|v| v * v).sum::<f32>().sqrt();
    let denom = norm(a) * norm(b);
    if denom == 0.0 { 0.0 } else { dot / denom }
}

fn embedding_cluster(candidates: Vec<String>, threshold: f32) -> SelfConsistencyResult {
    let total = candidates.len();
    if total == 0 {
        return SelfConsistencyResult {
            chosen: String::new(),
            support: 0,
            samples: 0,
            agreement: 0.0,
            candidates,
        };
    }

    let vecs: Vec<HashMap<String, f32>> = candidates.iter().map(|c| tokenize(c)).collect();

    let (best_idx, best_support) = (0..total)
        .map(|i| {
            let support = (0..total)
                .filter(|&j| cosine(&vecs[i], &vecs[j]) >= threshold)
                .count();
            (i, support)
        })
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .unwrap_or((0, 1));

    SelfConsistencyResult {
        chosen: candidates[best_idx].clone(),
        support: best_support,
        samples: total,
        agreement: best_support as f32 / total as f32,
        candidates,
    }
}
