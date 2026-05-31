// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let x = f64::from(*x);
        let y = f64::from(*y);
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if !denom.is_finite() || denom < f64::EPSILON {
        return 0.0;
    }

    let raw = dot / denom;
    if !raw.is_finite() {
        return 0.0;
    }

    #[allow(clippy::cast_possible_truncation)]
    let sim = raw.clamp(0.0, 1.0) as f32;
    sim
}

pub fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &f in v {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

pub fn bytes_to_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
            f32::from_le_bytes(arr)
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ScoredResult {
    pub id: String,
    pub vector_score: Option<f32>,
    pub keyword_score: Option<f32>,
    pub final_score: f32,
}

pub fn hybrid_merge(
    vector_results: &[(String, f32)],
    keyword_results: &[(String, f32)],
    vector_weight: f32,
    keyword_weight: f32,
    limit: usize,
) -> Vec<ScoredResult> {
    use std::collections::HashMap;

    let mut map: HashMap<String, ScoredResult> = HashMap::new();

    for (id, score) in vector_results {
        map.entry(id.clone())
            .and_modify(|r| r.vector_score = Some(*score))
            .or_insert_with(|| ScoredResult {
                id: id.clone(),
                vector_score: Some(*score),
                keyword_score: None,
                final_score: 0.0,
            });
    }

    let max_kw = keyword_results
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0_f32, f32::max);
    let max_kw = if max_kw < f32::EPSILON { 1.0 } else { max_kw };

    for (id, score) in keyword_results {
        let normalized = score / max_kw;
        map.entry(id.clone())
            .and_modify(|r| r.keyword_score = Some(normalized))
            .or_insert_with(|| ScoredResult {
                id: id.clone(),
                vector_score: None,
                keyword_score: Some(normalized),
                final_score: 0.0,
            });
    }

    let mut results: Vec<ScoredResult> = map
        .into_values()
        .map(|mut r| {
            let vs = r.vector_score.unwrap_or(0.0);
            let ks = r.keyword_score.unwrap_or(0.0);
            r.final_score = vector_weight * vs + keyword_weight * ks;
            r
        })
        .collect();

    results.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    results.truncate(limit);
    results
}
