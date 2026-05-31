// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use parking_lot::RwLock;

use super::super::vector::index::{LinearIndex, VectorIndex};

pub struct ShardedVectorIndex {
    shards: Vec<Arc<RwLock<LinearIndex>>>,
}

impl ShardedVectorIndex {

    pub fn new(shard_count: usize) -> Self {
        let bounded = shard_count.clamp(1, 128);
        let shards = (0..bounded)
            .map(|_| Arc::new(RwLock::new(LinearIndex::new())))
            .collect();
        Self { shards }
    }

    pub fn with_cpu_count() -> Self {
        let n = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4)
            .min(16);
        Self::new(n)
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    fn pick_shard(&self, id: &str) -> usize {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        id.hash(&mut h);
        (h.finish() as usize) % self.shards.len()
    }
}

impl Default for ShardedVectorIndex {
    fn default() -> Self {
        Self::with_cpu_count()
    }
}

impl VectorIndex for ShardedVectorIndex {
    fn upsert(&mut self, id: &str, embedding: &[f32]) {
        let idx = self.pick_shard(id);
        self.shards[idx].write().upsert(id, embedding);
    }

    fn remove(&mut self, id: &str) {

        let idx = self.pick_shard(id);
        self.shards[idx].write().remove(id);
    }

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)> {
        if limit == 0 || self.shards.is_empty() {
            return Vec::new();
        }

        use std::thread;
        let shard_results: Vec<_> = thread::scope(|s| {
            let handles: Vec<_> = self
                .shards
                .iter()
                .map(|shard| {
                    let shard = shard.clone();
                    let query_owned = query.to_vec();
                    s.spawn(move || {
                        let guard = shard.read();
                        guard.search(&query_owned, limit)
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .collect()
        });

        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        let mut heap: BinaryHeap<Reverse<(ordered_float::OrderedFloat<f32>, String)>> =
            BinaryHeap::with_capacity(limit + 1);

        for shard_hits in shard_results {
            for (id, sim) in shard_hits {
                heap.push(Reverse((ordered_float::OrderedFloat(sim), id)));
                if heap.len() > limit {
                    heap.pop();
                }
            }
        }

        let mut out: Vec<(String, f32)> = heap
            .into_iter()
            .map(|Reverse((of, id))| (id, of.0))
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    fn len(&self) -> usize {
        self.shards.iter().map(|s| s.read().len()).sum()
    }

    fn backend_name(&self) -> &'static str {

        "sharded"
    }
}
