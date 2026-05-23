// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

#[async_trait]
pub trait SampleProducer: Send + Sync {
    async fn sample(&self, prompt: &str, temperature: f32) -> anyhow::Result<String>;
}

#[derive(Debug, Clone)]
pub enum Aggregator {
    MajorityVote,
    EmbeddingCluster { similarity_threshold: f32 },
}

impl Aggregator {
    pub fn majority_vote() -> Self {
        Aggregator::MajorityVote
    }

    pub fn embedding_cluster(threshold: f32) -> Self {
        Aggregator::EmbeddingCluster {
            similarity_threshold: threshold,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SelfConsistencyResult {
    pub chosen: String,

    pub support: usize,

    pub samples: usize,

    pub agreement: f32,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SelfConsistencyConfig {
    pub samples: u32,

    pub temperature_jitter: f32,
    pub base_temperature: f32,
    pub aggregator: Aggregator,
}

impl Default for SelfConsistencyConfig {
    fn default() -> Self {
        Self {
            samples: 1,
            temperature_jitter: 0.0,
            base_temperature: 0.7,
            aggregator: Aggregator::MajorityVote,
        }
    }
}

pub struct SelfConsistency {
    config: SelfConsistencyConfig,
    producer: Arc<dyn SampleProducer>,
}

impl SelfConsistency {
    pub fn new(config: SelfConsistencyConfig, producer: Arc<dyn SampleProducer>) -> Self {
        Self { config, producer }
    }

    pub fn samples(&self) -> u32 {
        self.config.samples
    }

    pub async fn run(&self, prompt: &str) -> anyhow::Result<SelfConsistencyResult> {
        let n = self.config.samples.max(1) as usize;
        let temps = sample_temperatures(
            self.config.base_temperature,
            self.config.temperature_jitter,
            n,
        );

        let futs: Vec<_> = temps
            .into_iter()
            .map(|t| {
                let producer = self.producer.clone();
                let prompt = prompt.to_string();
                async move { producer.sample(&prompt, t).await }
            })
            .collect();

        let mut candidates: Vec<String> = Vec::with_capacity(n);
        for fut in futs {
            candidates.push(fut.await?);
        }

        Ok(aggregate(&self.config.aggregator, candidates))
    }
}

fn sample_temperatures(base: f32, jitter: f32, n: usize) -> Vec<f32> {
    if n <= 1 || jitter == 0.0 {
        return vec![base; n];
    }
    let step = jitter / ((n as f32 - 1.0).max(1.0));
    (0..n)
        .map(|i| base - jitter / 2.0 + step * i as f32)
        .collect()
}

pub fn aggregate(aggregator: &Aggregator, candidates: Vec<String>) -> SelfConsistencyResult {
    match aggregator {
        Aggregator::MajorityVote => majority_vote(candidates),
        Aggregator::EmbeddingCluster {
            similarity_threshold,
        } => embedding_cluster(candidates, *similarity_threshold),
    }
}

fn majority_vote(candidates: Vec<String>) -> SelfConsistencyResult {
    let total = candidates.len();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for c in &candidates {
        *counts.entry(c.as_str()).or_insert(0) += 1;
    }
    let (winner, support) = counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(s, n)| (s.to_string(), n))
        .unwrap_or_default();

    SelfConsistencyResult {
        chosen: winner,
        support,
        samples: total,
        agreement: if total == 0 {
            0.0
        } else {
            support as f32 / total as f32
        },
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
        .max_by_key(|(_, s)| *s)
        .unwrap_or((0, 1));

    SelfConsistencyResult {
        chosen: candidates[best_idx].clone(),
        support: best_support,
        samples: total,
        agreement: best_support as f32 / total as f32,
        candidates,
    }
}
