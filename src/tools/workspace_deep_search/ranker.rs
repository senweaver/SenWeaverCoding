// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::chunker::Chunk;
use super::planner::{QueryIntent, QueryPlan};

pub fn rerank(chunks: &mut [Chunk], plan: &QueryPlan) {
    let intent_prior = |path: &std::path::Path| -> f32 {
        let s = path.to_string_lossy().to_lowercase();
        match plan.intent {
            QueryIntent::Configuration => {
                if s.ends_with(".toml") || s.ends_with(".yaml") || s.ends_with(".yml") || s.ends_with(".json") || s.contains("config") {
                    0.35
                } else {
                    0.0
                }
            }
            QueryIntent::Documentation => {
                if s.ends_with(".md") || s.contains("docs") || s.contains("readme") {
                    0.35
                } else {
                    0.0
                }
            }
            QueryIntent::Implementation => {
                if s.ends_with(".rs") || s.ends_with(".ts") || s.ends_with(".tsx") || s.ends_with(".js") || s.ends_with(".py") || s.ends_with(".go") {
                    0.20
                } else {
                    0.0
                }
            }
            QueryIntent::Usage => {
                if s.contains("example") || s.contains("usage") || s.contains("readme") {
                    0.25
                } else {
                    0.0
                }
            }
            QueryIntent::Concept => 0.0,
        }
    };
    let path_recency_prior = |path: &std::path::Path| -> f32 {
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return 0.0,
        };
        let Ok(modified) = metadata.modified() else {
            return 0.0;
        };
        let secs_since = std::time::SystemTime::now()
            .duration_since(modified)
            .map(|d| d.as_secs() as f32)
            .unwrap_or(f32::MAX);
        let days = secs_since / 86_400.0;
        if days < 7.0 {
            0.20
        } else if days < 30.0 {
            0.10
        } else if days < 180.0 {
            0.03
        } else {
            0.0
        }
    };
    for chunk in chunks.iter_mut() {
        let coverage = chunk.tokens_matched.len() as f32 / (plan.tokens.len().max(1) as f32);
        let length_penalty = (chunk.body.chars().count().max(1) as f32).ln().max(1.0);
        let intent = intent_prior(&chunk.path);
        let recency = path_recency_prior(&chunk.path);
        chunk.raw_score = chunk.raw_score / length_penalty
            + chunk.structural_boost
            + coverage * 0.55
            + intent
            + recency;
    }
    chunks.sort_by(|a, b| b.raw_score.partial_cmp(&a.raw_score).unwrap_or(std::cmp::Ordering::Equal));
}

pub fn merge_into(into: &mut Vec<Chunk>, additions: Vec<Chunk>) -> usize {
    let mut added = 0usize;
    for chunk in additions {
        let already = into.iter().any(|existing| {
            existing.path == chunk.path
                && (chunk.line_start.max(existing.line_start)
                    <= chunk.line_end.min(existing.line_end))
        });
        if already {
            continue;
        }
        into.push(chunk);
        added += 1;
    }
    into.sort_by(|a, b| b.raw_score.partial_cmp(&a.raw_score).unwrap_or(std::cmp::Ordering::Equal));
    added
}

pub fn take_top(chunks: Vec<Chunk>, max_results: usize) -> Vec<Chunk> {
    chunks.into_iter().take(max_results).collect()
}
