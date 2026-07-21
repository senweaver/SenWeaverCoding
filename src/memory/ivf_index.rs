// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;

use super::vector::index::VectorIndex;

pub const IVF_FILE_MAGIC: &[u8; 8] = b"SENIVF01";

#[derive(Debug, Clone)]
pub struct AdaptiveNprobe {

    pub recall_floor: f32,

    pub recall_ceiling: f32,

    pub max_nprobe: usize,

    pub min_nprobe: usize,

    window: VecDeque<f32>,

    pub window_size: usize,
}

impl Default for AdaptiveNprobe {
    fn default() -> Self {
        Self {
            recall_floor: 0.85,
            recall_ceiling: 0.95,
            max_nprobe: 64,
            min_nprobe: 1,
            window: VecDeque::with_capacity(100),
            window_size: 100,
        }
    }
}

impl AdaptiveNprobe {

    pub fn record_recall(&mut self, recall: f32) {
        if self.window.len() >= self.window_size {
            self.window.pop_front();
        }
        self.window.push_back(recall.clamp(0.0, 1.0));
    }

    pub fn mean_recall(&self) -> Option<f32> {
        if self.window.is_empty() {
            return None;
        }
        let sum: f32 = self.window.iter().sum();
        Some(sum / self.window.len() as f32)
    }

    pub fn propose_delta(&self, current_nprobe: usize) -> i32 {
        let Some(mean) = self.mean_recall() else {
            return 0;
        };
        if mean < self.recall_floor && current_nprobe < self.max_nprobe {
            1
        } else if mean > self.recall_ceiling && current_nprobe > self.min_nprobe {
            -1
        } else {
            0
        }
    }

    pub fn next_nprobe(&self, current_nprobe: usize) -> usize {
        let delta = self.propose_delta(current_nprobe);
        let proposed =
            (current_nprobe as i32 + delta).clamp(self.min_nprobe as i32, self.max_nprobe as i32);
        proposed as usize
    }

    pub fn window_len(&self) -> usize {
        self.window.len()
    }

    pub fn clear(&mut self) {
        self.window.clear();
    }
}

#[derive(Clone, Debug)]
struct IvfEntry {
    id: String,
    embedding: Vec<f32>,
    norm: f32,
}

pub struct IvfVectorIndex {

    centroids: Vec<Vec<f32>>,

    centroid_norms: Vec<f32>,

    inverted_lists: Vec<Vec<IvfEntry>>,

    num_clusters: usize,

    nprobe: usize,

    dim: Option<usize>,

    total: usize,

    kmeans_iters: usize,

    upserts_since_train: usize,
}

impl IvfVectorIndex {

    pub fn new(num_clusters: usize, nprobe: usize) -> Self {
        let nc = num_clusters.clamp(1, 1024);
        let np = nprobe.clamp(1, nc);
        Self {
            centroids: Vec::with_capacity(nc),
            centroid_norms: Vec::with_capacity(nc),
            inverted_lists: (0..nc).map(|_| Vec::new()).collect(),
            num_clusters: nc,
            nprobe: np,
            dim: None,
            total: 0,
            kmeans_iters: 12,
            upserts_since_train: 0,
        }
    }

    pub fn for_size(n: usize) -> Self {
        let nc = ((n as f64).sqrt() as usize).clamp(8, 1024);
        let np = ((nc as f64).sqrt() as usize).clamp(1, nc);
        Self::new(nc, np)
    }

    pub fn num_clusters(&self) -> usize {
        self.num_clusters
    }

    pub fn nprobe(&self) -> usize {
        self.nprobe
    }

    pub fn set_nprobe(&mut self, n: usize) {
        self.nprobe = n.clamp(1, self.num_clusters);
    }

    pub fn is_trained(&self) -> bool {
        !self.centroids.is_empty()
    }

    pub fn train(&mut self, data: &[Vec<f32>]) -> f32 {
        if data.is_empty() {
            return 0.0;
        }
        let d = data[0].len();
        self.dim = Some(d);

        self.centroids.clear();
        self.centroid_norms.clear();

        let mut rng_state: u64 = (data.len() as u64).wrapping_mul(2_862_933_555_777_941_757)
            ^ ((d as u64).wrapping_add(1_442_695_040_888_963_407));
        let mut next_rand = || -> f32 {

            rng_state ^= rng_state >> 12;
            rng_state ^= rng_state << 25;
            rng_state ^= rng_state >> 27;
            (rng_state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as f32 / (1_u64 << 31) as f32
        };

        let first_idx = (next_rand() * data.len() as f32) as usize % data.len();
        self.centroids.push(data[first_idx].clone());
        self.centroid_norms.push(compute_norm(&data[first_idx]));

        let mut min_dist_sq: Vec<f32> = data
            .iter()
            .map(|v| squared_euclidean(v, &self.centroids[0]))
            .collect();

        while self.centroids.len() < self.num_clusters {
            let total: f32 = min_dist_sq.iter().sum();
            if total < f32::EPSILON {

                let fallback = data[self.centroids.len() % data.len()].clone();
                self.centroid_norms.push(compute_norm(&fallback));
                self.centroids.push(fallback);
                continue;
            }
            let target = next_rand() * total;
            let mut acc = 0.0_f32;
            let mut chosen = data.len() - 1;
            for (i, &dsq) in min_dist_sq.iter().enumerate() {
                acc += dsq;
                if acc >= target {
                    chosen = i;
                    break;
                }
            }
            let new_c = data[chosen].clone();
            self.centroid_norms.push(compute_norm(&new_c));
            self.centroids.push(new_c.clone());

            for (i, v) in data.iter().enumerate() {
                let d2 = squared_euclidean(v, &new_c);
                if d2 < min_dist_sq[i] {
                    min_dist_sq[i] = d2;
                }
            }
        }

        for _iter in 0..self.kmeans_iters {
            let mut sums: Vec<Vec<f32>> = (0..self.num_clusters).map(|_| vec![0.0; d]).collect();
            let mut counts: Vec<usize> = vec![0; self.num_clusters];

            for vec in data {
                let (cid, _) = self.nearest_centroid(vec);
                for (s, v) in sums[cid].iter_mut().zip(vec.iter()) {
                    *s += *v;
                }
                counts[cid] += 1;
            }

            for cid in 0..self.num_clusters {
                if counts[cid] > 0 {
                    let inv = 1.0 / counts[cid] as f32;
                    for s in sums[cid].iter_mut() {
                        *s *= inv;
                    }
                    self.centroids[cid] = std::mem::take(&mut sums[cid]);
                    self.centroid_norms[cid] = compute_norm(&self.centroids[cid]);
                }

            }
        }

        let mut inertia = 0.0f32;
        for vec in data {
            let (_, sim) = self.nearest_centroid(vec);

            inertia += (1.0 - sim).max(0.0) * 2.0;
        }
        inertia
    }

    fn ensure_trained_with(&mut self, sample: &[f32]) {
        if self.is_trained() {
            return;
        }
        if self.dim.is_none() {
            self.dim = Some(sample.len());
        }

        // Seed with a SINGLE real centroid. Previously this padded with N-1 zero
        // centroids, which `nearest_centroid` skips, so every vector collapsed
        // into cluster 0 until the first full retrain at 256 upserts. Real
        // multi-cluster structure now forms via `maybe_seed_retrain` as soon as
        // enough vectors accumulate.
        self.centroids.push(sample.to_vec());
        self.centroid_norms.push(compute_norm(sample));
    }

    /// During the seed period the index has a single centroid. Once enough
    /// vectors have accumulated to form real clusters, run one k-means pass so
    /// searches stop scanning a single giant list. Cheap no-op after the index
    /// has grown past the seed threshold.
    fn maybe_seed_retrain(&mut self) {
        let seed_threshold = self.num_clusters.max(8);
        if self.centroids.len() < self.num_clusters && self.total >= seed_threshold {
            self.retrain_from_contents();
        }
    }

    fn nearest_centroid(&self, v: &[f32]) -> (usize, f32) {
        let v_norm = compute_norm(v);
        if v_norm < f32::EPSILON {
            return (0, 0.0);
        }
        let mut best_idx = 0usize;
        let mut best_sim = f32::MIN;
        for (i, (c, &c_norm)) in self
            .centroids
            .iter()
            .zip(self.centroid_norms.iter())
            .enumerate()
        {
            if c_norm < f32::EPSILON {
                continue;
            }
            let dot: f32 = v.iter().zip(c.iter()).map(|(a, b)| a * b).sum();
            let sim = dot / (v_norm * c_norm);
            if sim > best_sim {
                best_sim = sim;
                best_idx = i;
            }
        }
        (best_idx, best_sim)
    }

    fn top_clusters(&self, query: &[f32], k: usize) -> Vec<usize> {
        let q_norm = compute_norm(query);
        if q_norm < f32::EPSILON {
            return Vec::new();
        }
        let mut scored: Vec<(f32, usize)> = self
            .centroids
            .iter()
            .zip(self.centroid_norms.iter())
            .enumerate()
            .map(|(i, (c, &c_norm))| {
                if c_norm < f32::EPSILON {
                    return (f32::MIN, i);
                }
                let dot: f32 = query.iter().zip(c.iter()).map(|(a, b)| a * b).sum();
                let sim = dot / (q_norm * c_norm);
                (sim, i)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
        scored.truncate(k);
        scored.into_iter().map(|(_, i)| i).collect()
    }

    fn find_cluster(&self, id: &str) -> Option<(usize, usize)> {
        for (cid, list) in self.inverted_lists.iter().enumerate() {
            if let Some(pos) = list.iter().position(|e| e.id == id) {
                return Some((cid, pos));
            }
        }
        None
    }

    pub fn needs_retraining(&self) -> bool {
        if self.total == 0 || self.num_clusters <= 1 {
            return false;
        }
        let avg = self.total as f32 / self.num_clusters as f32;
        let threshold = (avg * 2.0).max(16.0);
        self.inverted_lists
            .iter()
            .any(|l| l.len() as f32 > threshold)
    }

    fn retrain_from_contents(&mut self) {
        let entries: Vec<IvfEntry> = self
            .inverted_lists
            .iter_mut()
            .flat_map(|l| l.drain(..))
            .collect();
        if entries.is_empty() {
            return;
        }
        let data: Vec<Vec<f32>> = entries.iter().map(|e| e.embedding.clone()).collect();
        self.train(&data);
        self.inverted_lists = (0..self.num_clusters).map(|_| Vec::new()).collect();
        self.total = 0;
        for entry in entries {
            let (cid, _) = self.nearest_centroid(&entry.embedding);
            self.inverted_lists[cid].push(entry);
            self.total += 1;
        }
    }

    pub fn list_sizes(&self) -> Vec<usize> {
        let mut sizes: Vec<usize> = self.inverted_lists.iter().map(Vec::len).collect();
        sizes.sort_by(|a, b| b.cmp(a));
        sizes
    }

    pub fn save_to_writer<W: Write>(&self, w: &mut W) -> std::io::Result<usize> {
        let mut total = 0usize;
        w.write_all(IVF_FILE_MAGIC)?;
        total += IVF_FILE_MAGIC.len();

        write_u64(w, self.num_clusters as u64)?;
        write_u64(w, self.nprobe as u64)?;
        write_u64(w, self.dim.unwrap_or(0) as u64)?;
        write_u64(w, self.total as u64)?;
        total += 32;

        for (centroid, norm) in self.centroids.iter().zip(self.centroid_norms.iter()) {
            write_u64(w, centroid.len() as u64)?;
            for &x in centroid {
                w.write_all(&x.to_le_bytes())?;
            }
            w.write_all(&norm.to_le_bytes())?;
            total += 8 + centroid.len() * 4 + 4;
        }

        for list in &self.inverted_lists {
            write_u64(w, list.len() as u64)?;
            total += 8;
            for entry in list {
                let id_bytes = entry.id.as_bytes();
                write_u64(w, id_bytes.len() as u64)?;
                w.write_all(id_bytes)?;
                write_u64(w, entry.embedding.len() as u64)?;
                for &x in &entry.embedding {
                    w.write_all(&x.to_le_bytes())?;
                }
                w.write_all(&entry.norm.to_le_bytes())?;
                total += 8 + id_bytes.len() + 8 + entry.embedding.len() * 4 + 4;
            }
        }

        Ok(total)
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> std::io::Result<usize> {
        let path = path.as_ref();
        let tmp = path.with_extension("tmp");
        let written = {
            let mut f = std::fs::File::create(&tmp)?;
            let n = self.save_to_writer(&mut f)?;
            f.flush()?;
            n
        };
        std::fs::rename(&tmp, path)?;
        Ok(written)
    }

    pub fn load_from_reader<R: Read>(r: &mut R) -> std::io::Result<Self> {
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != IVF_FILE_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "bad IVF magic: expected {:?}, got {:?}",
                    std::str::from_utf8(IVF_FILE_MAGIC).unwrap_or("?"),
                    std::str::from_utf8(&magic).unwrap_or("?")
                ),
            ));
        }
        let num_clusters = read_u64(r)? as usize;
        let nprobe = read_u64(r)? as usize;
        let dim_stored = read_u64(r)? as usize;
        let total = read_u64(r)? as usize;

        const MAX_SANE_CLUSTERS: usize = 1 << 20;
        const MAX_SANE_DIM: usize = 1 << 16;
        const MAX_SANE_TOTAL: usize = 1 << 28;
        if num_clusters == 0
            || num_clusters > MAX_SANE_CLUSTERS
            || dim_stored > MAX_SANE_DIM
            || total > MAX_SANE_TOTAL
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "IVF snapshot header out of bounds: clusters={num_clusters}, dim={dim_stored}, total={total}"
                ),
            ));
        }

        let mut index = IvfVectorIndex::new(num_clusters, nprobe);
        index.dim = if dim_stored == 0 {
            None
        } else {
            Some(dim_stored)
        };
        index.total = total;
        index.centroids.clear();
        index.centroid_norms.clear();

        for _ in 0..num_clusters {
            let len = read_u64(r)? as usize;
            let mut centroid = vec![0.0_f32; len];
            for slot in centroid.iter_mut() {
                let mut buf = [0u8; 4];
                r.read_exact(&mut buf)?;
                *slot = f32::from_le_bytes(buf);
            }
            let mut nbuf = [0u8; 4];
            r.read_exact(&mut nbuf)?;
            index.centroids.push(centroid);
            index.centroid_norms.push(f32::from_le_bytes(nbuf));
        }

        index.inverted_lists = (0..num_clusters).map(|_| Vec::new()).collect();
        for list in index.inverted_lists.iter_mut() {
            let entries = read_u64(r)? as usize;
            for _ in 0..entries {
                let id_len = read_u64(r)? as usize;
                let mut id_bytes = vec![0u8; id_len];
                r.read_exact(&mut id_bytes)?;
                let id = String::from_utf8(id_bytes).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid UTF-8 id")
                })?;
                let emb_len = read_u64(r)? as usize;
                let mut embedding = vec![0.0_f32; emb_len];
                for slot in embedding.iter_mut() {
                    let mut buf = [0u8; 4];
                    r.read_exact(&mut buf)?;
                    *slot = f32::from_le_bytes(buf);
                }
                let mut nbuf = [0u8; 4];
                r.read_exact(&mut nbuf)?;
                list.push(IvfEntry {
                    id,
                    embedding,
                    norm: f32::from_le_bytes(nbuf),
                });
            }
        }
        Ok(index)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut f = std::fs::File::open(path)?;
        Self::load_from_reader(&mut f)
    }

    #[must_use]
    pub fn dimensions(&self) -> Option<usize> {
        self.dim
    }

    #[must_use]
    pub fn entry_ids(&self) -> Vec<String> {
        self.inverted_lists
            .iter()
            .flat_map(|list| list.iter().map(|e| e.id.clone()))
            .collect()
    }
}

#[inline]
fn write_u64<W: Write>(w: &mut W, v: u64) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

#[inline]
fn read_u64<R: Read>(r: &mut R) -> std::io::Result<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

fn compute_norm(v: &[f32]) -> f32 {

    dot_product_unrolled(v, v).sqrt()
}

#[inline]
pub(crate) fn dot_product_unrolled(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let chunks = n / 4;
    let remainder = n % 4;

    let mut acc0 = 0.0_f32;
    let mut acc1 = 0.0_f32;
    let mut acc2 = 0.0_f32;
    let mut acc3 = 0.0_f32;

    for i in 0..chunks {
        let base = i * 4;

        acc0 += a[base] * b[base];
        acc1 += a[base + 1] * b[base + 1];
        acc2 += a[base + 2] * b[base + 2];
        acc3 += a[base + 3] * b[base + 3];
    }
    let base = chunks * 4;
    for i in 0..remainder {
        acc0 += a[base + i] * b[base + i];
    }
    acc0 + acc1 + acc2 + acc3
}

#[inline]
pub fn cosine_similarity_fast(a: &[f32], b: &[f32]) -> f32 {
    let dot = dot_product_unrolled(a, b);
    let norm_a = dot_product_unrolled(a, a).sqrt();
    let norm_b = dot_product_unrolled(b, b).sqrt();
    if norm_a < f32::EPSILON || norm_b < f32::EPSILON {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

fn squared_euclidean(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0_f32;
    let n = a.len().min(b.len());
    for i in 0..n {
        let d = a[i] - b[i];
        sum += d * d;
    }
    sum
}

impl VectorIndex for IvfVectorIndex {
    fn upsert(&mut self, id: &str, embedding: &[f32]) {
        const RETRAIN_CHECK_EVERY: usize = 256;
        if let Some(dim) = self.dim {
            if embedding.len() != dim {
                crate::observability::coordination_metrics::incr_vector_dim_mismatch();
                tracing::warn!(
                    target: "memory.vector",
                    id,
                    expected = dim,
                    got = embedding.len(),
                    "skipping vector upsert with mismatched dimensions (embedding model changed?)"
                );
                return;
            }
        }
        self.ensure_trained_with(embedding);

        if let Some((cid, pos)) = self.find_cluster(id) {
            self.inverted_lists[cid].swap_remove(pos);
            self.total = self.total.saturating_sub(1);
        }

        let (cid, _) = self.nearest_centroid(embedding);
        let entry = IvfEntry {
            id: id.to_string(),
            embedding: embedding.to_vec(),
            norm: compute_norm(embedding),
        };
        self.inverted_lists[cid].push(entry);
        self.total += 1;

        // Promote from the single-centroid seed state to real clusters early,
        // instead of waiting for the 256-upsert retrain check.
        self.maybe_seed_retrain();

        self.upserts_since_train += 1;
        if self.upserts_since_train >= RETRAIN_CHECK_EVERY {
            self.upserts_since_train = 0;
            if self.needs_retraining() {
                self.retrain_from_contents();
            }
        }
    }

    fn remove(&mut self, id: &str) {
        if let Some((cid, pos)) = self.find_cluster(id) {
            self.inverted_lists[cid].swap_remove(pos);
            self.total = self.total.saturating_sub(1);
        }
    }

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)> {
        if limit == 0 || self.total == 0 || !self.is_trained() {
            return Vec::new();
        }
        if let Some(dim) = self.dim {
            if query.len() != dim {
                crate::observability::coordination_metrics::incr_vector_dim_mismatch();
                tracing::warn!(
                    target: "memory.vector",
                    expected = dim,
                    got = query.len(),
                    "skipping vector search with mismatched query dimensions (embedding model changed?)"
                );
                return Vec::new();
            }
        }
        let q_norm = compute_norm(query);
        if q_norm < f32::EPSILON {
            return Vec::new();
        }

        let probe_ids = self.top_clusters(query, self.nprobe);

        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        let mut heap: BinaryHeap<Reverse<(ordered_float::OrderedFloat<f32>, String)>> =
            BinaryHeap::with_capacity(limit + 1);
        let mut min_sim: f32 = f32::MIN;

        for &cid in &probe_ids {
            let list = &self.inverted_lists[cid];
            for e in list {
                if e.norm < f32::EPSILON {
                    continue;
                }
                let dot: f32 = query
                    .iter()
                    .zip(e.embedding.iter())
                    .map(|(a, b)| a * b)
                    .sum();
                let sim = dot / (q_norm * e.norm);
                if heap.len() >= limit && sim <= min_sim {
                    continue;
                }
                heap.push(Reverse((ordered_float::OrderedFloat(sim), e.id.clone())));
                if heap.len() > limit {
                    heap.pop();
                    if let Some(Reverse((of, _))) = heap.peek() {
                        min_sim = of.0;
                    }
                }
            }
        }

        let mut out: Vec<(String, f32)> = heap
            .into_iter()
            .map(|Reverse((of, id))| (id, of.0))
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
        out
    }

    fn len(&self) -> usize {
        self.total
    }

    fn backend_name(&self) -> &'static str {
        "ivf"
    }
}
