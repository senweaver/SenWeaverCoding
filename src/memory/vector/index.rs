// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub trait VectorIndex: Send + Sync {

    fn upsert(&mut self, id: &str, embedding: &[f32]);

    fn remove(&mut self, id: &str);

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)>;

    fn len(&self) -> usize;

    fn backend_name(&self) -> &'static str;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct LinearIndex {

    ids: Vec<String>,
    embeddings: Vec<Vec<f32>>,
    norms: Vec<f32>,
}

impl LinearIndex {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            embeddings: Vec::new(),
            norms: Vec::new(),
        }
    }

    fn compute_norm(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    fn find_position(&self, id: &str) -> Option<usize> {
        self.ids.iter().position(|existing| existing == id)
    }
}

impl Default for LinearIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorIndex for LinearIndex {
    fn upsert(&mut self, id: &str, embedding: &[f32]) {
        let norm = Self::compute_norm(embedding);
        match self.find_position(id) {
            Some(idx) => {
                self.embeddings[idx] = embedding.to_vec();
                self.norms[idx] = norm;
            }
            None => {
                self.ids.push(id.to_string());
                self.embeddings.push(embedding.to_vec());
                self.norms.push(norm);
            }
        }
    }

    fn remove(&mut self, id: &str) {
        if let Some(idx) = self.find_position(id) {
            self.ids.swap_remove(idx);
            self.embeddings.swap_remove(idx);
            self.norms.swap_remove(idx);
        }
    }

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)> {
        if limit == 0 || self.ids.is_empty() {
            return Vec::new();
        }

        let query_norm = Self::compute_norm(query);
        if query_norm < f32::EPSILON {
            return Vec::new();
        }

        let mut heap: BinaryHeap<Reverse<(ordered_float::OrderedFloat<f32>, usize)>> =
            BinaryHeap::with_capacity(limit + 1);
        let mut min_sim: f32 = f32::MIN;
        let query_dim = query.len();

        for (idx, emb) in self.embeddings.iter().enumerate() {
            if emb.len() != query_dim {
                continue;
            }
            let dot: f32 = query.iter().zip(emb.iter()).map(|(a, b)| a * b).sum();
            let emb_norm = self.norms[idx];
            if emb_norm < f32::EPSILON {
                continue;
            }
            let sim = dot / (query_norm * emb_norm);
            if heap.len() >= limit && sim <= min_sim {
                continue;
            }
            heap.push(Reverse((ordered_float::OrderedFloat(sim), idx)));
            if heap.len() > limit {
                heap.pop();
                if let Some(Reverse((of, _))) = heap.peek() {
                    min_sim = of.0;
                }
            }
        }

        let mut out: Vec<(String, f32)> = heap
            .into_iter()
            .map(|Reverse((of, idx))| (self.ids[idx].clone(), of.0))
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    fn len(&self) -> usize {
        self.ids.len()
    }

    fn backend_name(&self) -> &'static str {
        "linear"
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum VectorBackend {
    Linear,
    Sharded,
    Ivf,
    Hnsw,
}

impl Default for VectorBackend {
    fn default() -> Self {
        VectorBackend::Ivf
    }
}

impl VectorBackend {

    pub fn from_str_lenient(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "linear" | "brute" | "bruteforce" => Some(Self::Linear),
            "sharded" | "shard" | "parallel" => Some(Self::Sharded),
            "ivf" | "inverted" | "clustered" | "default" => Some(Self::Ivf),
            "hnsw" | "graph" => Some(Self::Hnsw),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Sharded => "sharded",
            Self::Ivf => "ivf",
            Self::Hnsw => "hnsw",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Linear => "brute-force cosine (best for < 1k vectors)",
            Self::Sharded => "parallel fan-out across CPU shards (~10k sweet spot)",
            Self::Ivf => "inverted-file clustering, sqrt(N) probe (default, ~100k+)",
            Self::Hnsw => "hierarchical navigable small world graph (log(N) search, high recall)",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[Self::Linear, Self::Sharded, Self::Ivf, Self::Hnsw]
    }
}

pub fn build_backend(kind: VectorBackend) -> Box<dyn VectorIndex> {
    match kind {
        VectorBackend::Linear => Box::new(LinearIndex::new()),
        VectorBackend::Sharded => {
            Box::new(crate::memory::sharded::index::ShardedVectorIndex::with_cpu_count())
        }
        VectorBackend::Ivf => Box::new(crate::memory::ivf_index::IvfVectorIndex::for_size(10_000)),
        VectorBackend::Hnsw => Box::new(crate::memory::hnsw::HnswMemIndex::new()),
    }
}

pub fn build_default_backend() -> Box<dyn VectorIndex> {
    let kind = crate::services::try_get_services()
        .map(|svc| {
            let cfg = svc.shared_config.load();
            cfg.memory.vector_backend.unwrap_or_default()
        })
        .unwrap_or_default();
    build_backend(kind)
}
