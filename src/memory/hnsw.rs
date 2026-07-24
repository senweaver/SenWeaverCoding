// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use super::vector::index::VectorIndex;

#[derive(Debug, Clone, Copy, PartialEq)]
struct SimScore(f32);

impl Eq for SimScore {}
impl Ord for SimScore {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for SimScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone)]
struct Node {
    id: String,
    vector: Vec<f32>,
    norm: f32,

    neighbors: Vec<Vec<usize>>,

    deleted: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct HnswParams {

    pub m: usize,

    pub m_max0: usize,

    pub ef_construction: usize,

    pub ef_search: usize,

    pub rng_seed: u64,
}

impl Default for HnswParams {
    fn default() -> Self {
        Self {
            m: 16,
            m_max0: 32,
            ef_construction: 100,
            ef_search: 50,
            rng_seed: 0x517cc1b727220a95,
        }
    }
}

#[derive(Clone)]
pub struct HnswMemIndex {
    params: HnswParams,

    ml: f64,
    nodes: Vec<Node>,
    id_to_index: HashMap<String, usize>,
    entry_point: Option<usize>,
    top_level: usize,

    rng_state: u64,

    deleted_count: usize,
}

impl HnswMemIndex {
    pub fn new() -> Self {
        Self::with_params(HnswParams::default())
    }

    pub fn with_params(params: HnswParams) -> Self {
        let ml = 1.0 / (params.m as f64).ln();
        Self {
            params,
            ml,
            nodes: Vec::new(),
            id_to_index: HashMap::new(),
            entry_point: None,
            top_level: 0,
            rng_state: params.rng_seed.max(1),
            deleted_count: 0,
        }
    }

    pub fn tombstone_ratio(&self) -> f32 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        self.deleted_count as f32 / self.nodes.len() as f32
    }

    fn next_rand(&mut self) -> u64 {

        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        x.wrapping_mul(0x2545f4914f6cdd1d)
    }

    fn next_level(&mut self) -> usize {

        let u = (self.next_rand() >> 1) as f64 / ((1u64 << 63) as f64);
        let u = u.max(f64::MIN_POSITIVE);
        (-u.ln() * self.ml) as usize
    }

    fn compute_norm(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    #[inline]
    fn cosine(query: &[f32], qnorm: f32, other: &[f32], onorm: f32) -> f32 {
        if qnorm < f32::EPSILON || onorm < f32::EPSILON {
            return 0.0;
        }
        let dot: f32 = query.iter().zip(other.iter()).map(|(a, b)| a * b).sum();
        dot / (qnorm * onorm)
    }

    fn score_to(&self, q: &[f32], qnorm: f32, idx: usize) -> f32 {
        let n = &self.nodes[idx];
        Self::cosine(q, qnorm, &n.vector, n.norm)
    }

    fn search_layer_greedy(
        &self,
        q: &[f32],
        qnorm: f32,
        entry: usize,
        layer: usize,
    ) -> (usize, f32) {
        let mut best = entry;
        let mut best_sim = self.score_to(q, qnorm, entry);
        loop {
            let mut improved = false;
            let neighbours = &self.nodes[best].neighbors[layer];
            for &n in neighbours {
                if self.nodes[n].deleted {
                    continue;
                }
                let sim = self.score_to(q, qnorm, n);
                if sim > best_sim {
                    best_sim = sim;
                    best = n;
                    improved = true;
                }
            }
            if !improved {
                return (best, best_sim);
            }
        }
    }

    fn search_layer_ef(
        &self,
        q: &[f32],
        qnorm: f32,
        entries: &[usize],
        layer: usize,
        ef: usize,
    ) -> Vec<(usize, f32)> {
        let mut visited: HashSet<usize> = HashSet::with_capacity(ef * 4);

        let mut frontier: BinaryHeap<(SimScore, usize)> = BinaryHeap::new();

        let mut best: BinaryHeap<(SimScore, usize)> = BinaryHeap::new();

        for &e in entries {
            if visited.insert(e) {
                let s = self.score_to(q, qnorm, e);
                frontier.push((SimScore(s), e));
                best.push((SimScore(-s), e));

                if best.len() > ef {
                    best.pop();
                }
            }
        }

        while let Some((SimScore(cur_sim), cur)) = frontier.pop() {

            if let Some(&(SimScore(worst_neg), _)) = best.peek() {
                let worst_in_best = -worst_neg;
                if best.len() >= ef && cur_sim < worst_in_best {
                    break;
                }
            }

            for &n in &self.nodes[cur].neighbors[layer] {
                if !visited.insert(n) {
                    continue;
                }
                let s = self.score_to(q, qnorm, n);
                let should_push_frontier = match best.peek() {
                    Some(&(SimScore(worst_neg), _)) => {
                        let worst = -worst_neg;
                        best.len() < ef || s > worst
                    }
                    None => true,
                };
                if should_push_frontier {
                    frontier.push((SimScore(s), n));
                    best.push((SimScore(-s), n));
                    if best.len() > ef {
                        best.pop();
                    }
                }
            }
        }

        let mut out: Vec<(usize, f32)> = best
            .into_iter()
            .map(|(SimScore(neg), i)| (i, -neg))
            .filter(|(i, _)| !self.nodes[*i].deleted)
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
        out
    }

    fn select_neighbours(candidates: &[(usize, f32)], m: usize) -> Vec<usize> {
        candidates.iter().take(m).map(|&(i, _)| i).collect()
    }

    fn m_for_layer(&self, layer: usize) -> usize {
        if layer == 0 {
            self.params.m_max0
        } else {
            self.params.m
        }
    }

    fn connect_bidirectional(&mut self, node_idx: usize, neighbours: &[usize], layer: usize) {

        self.nodes[node_idx].neighbors[layer].extend_from_slice(neighbours);

        let m = self.m_for_layer(layer);
        let node_vec = self.nodes[node_idx].vector.clone();
        let node_norm = self.nodes[node_idx].norm;
        for &n in neighbours {
            self.nodes[n].neighbors[layer].push(node_idx);
            if self.nodes[n].neighbors[layer].len() > m {

                let n_vec = self.nodes[n].vector.clone();
                let n_norm = self.nodes[n].norm;
                let mut scored: Vec<(usize, f32)> = self.nodes[n].neighbors[layer]
                    .iter()
                    .map(|&x| {
                        let ox = &self.nodes[x];
                        (x, Self::cosine(&n_vec, n_norm, &ox.vector, ox.norm))
                    })
                    .collect();
                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
                self.nodes[n].neighbors[layer] = Self::select_neighbours(&scored, m);
            }
        }

        let mut seen: HashSet<usize> = HashSet::new();
        self.nodes[node_idx].neighbors[layer].retain(|&x| seen.insert(x));
        if self.nodes[node_idx].neighbors[layer].len() > m {

            let q = node_vec;
            let qnorm = node_norm;
            let mut scored: Vec<(usize, f32)> = self.nodes[node_idx].neighbors[layer]
                .iter()
                .map(|&x| {
                    let ox = &self.nodes[x];
                    (x, Self::cosine(&q, qnorm, &ox.vector, ox.norm))
                })
                .collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            self.nodes[node_idx].neighbors[layer] = Self::select_neighbours(&scored, m);
        }
    }
}

impl Default for HnswMemIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorIndex for HnswMemIndex {
    fn upsert(&mut self, id: &str, embedding: &[f32]) {

        if let Some(&idx) = self.id_to_index.get(id) {
            self.nodes[idx].vector = embedding.to_vec();
            self.nodes[idx].norm = Self::compute_norm(embedding);
            if self.nodes[idx].deleted {
                self.nodes[idx].deleted = false;
                self.deleted_count = self.deleted_count.saturating_sub(1);
            }
            return;
        }

        let norm = Self::compute_norm(embedding);
        let level = self.next_level();
        let new_idx = self.nodes.len();
        let mut neighbors: Vec<Vec<usize>> = Vec::with_capacity(level + 1);
        for _ in 0..=level {
            neighbors.push(Vec::new());
        }
        self.nodes.push(Node {
            id: id.to_string(),
            vector: embedding.to_vec(),
            norm,
            neighbors,
            deleted: false,
        });
        self.id_to_index.insert(id.to_string(), new_idx);

        let Some(entry) = self.entry_point else {
            self.entry_point = Some(new_idx);
            self.top_level = level;
            return;
        };

        let mut cur_ep = entry;
        let mut cur_level = self.top_level;
        let vec_q = self.nodes[new_idx].vector.clone();
        let qnorm = self.nodes[new_idx].norm;
        while cur_level > level {
            let (next, _) = self.search_layer_greedy(&vec_q, qnorm, cur_ep, cur_level);
            cur_ep = next;
            cur_level -= 1;
            if cur_level == 0 {
                break;
            }
        }

        let start = level.min(self.top_level);
        let mut ep_set: Vec<usize> = vec![cur_ep];
        for l in (0..=start).rev() {
            let candidates =
                self.search_layer_ef(&vec_q, qnorm, &ep_set, l, self.params.ef_construction);
            let m = self.m_for_layer(l);
            let selected = Self::select_neighbours(&candidates, m);
            self.connect_bidirectional(new_idx, &selected, l);

            ep_set = candidates.into_iter().map(|(i, _)| i).collect();
            if ep_set.is_empty() {
                ep_set.push(cur_ep);
            }
            if l == 0 {
                break;
            }
        }

        if level > self.top_level {
            self.entry_point = Some(new_idx);
            self.top_level = level;
        }
    }

    fn remove(&mut self, id: &str) {
        if let Some(&idx) = self.id_to_index.get(id) {
            if !self.nodes[idx].deleted {
                self.nodes[idx].deleted = true;
                self.deleted_count += 1;
            }
        }
    }

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)> {
        if limit == 0 || self.nodes.is_empty() {
            return Vec::new();
        }
        let Some(entry) = self.entry_point else {
            return Vec::new();
        };
        let qnorm = Self::compute_norm(query);
        if qnorm < f32::EPSILON {
            return Vec::new();
        }

        let mut cur_ep = entry;
        for l in (1..=self.top_level).rev() {
            let (next, _) = self.search_layer_greedy(query, qnorm, cur_ep, l);
            cur_ep = next;
        }

        let ef = self.params.ef_search.max(limit);
        let mut candidates = self.search_layer_ef(query, qnorm, &[cur_ep], 0, ef);
        candidates.truncate(limit);
        candidates
            .into_iter()
            .map(|(idx, score)| (self.nodes[idx].id.clone(), score))
            .collect()
    }

    fn len(&self) -> usize {
        self.nodes.len().saturating_sub(self.deleted_count)
    }

    fn backend_name(&self) -> &'static str {
        "hnsw"
    }
}
