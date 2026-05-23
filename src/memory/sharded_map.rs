// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

pub const DEFAULT_SHARDS: usize = 16;

pub struct ShardedMap<V> {
    shards: Vec<RwLock<HashMap<String, V>>>,
    hasher: RandomState,
}

impl<V> ShardedMap<V> {

    pub fn new() -> Self {
        Self::with_shards(DEFAULT_SHARDS)
    }

    pub fn with_shards(shards: usize) -> Self {
        let n = shards.max(1).next_power_of_two();
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(RwLock::new(HashMap::new()));
        }
        Self {
            shards: v,
            hasher: RandomState::new(),
        }
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    fn shard_index(&self, key: &str) -> usize {
        let mut h = self.hasher.build_hasher();
        h.write(key.as_bytes());
        let hash = h.finish() as usize;
        hash & (self.shards.len() - 1)
    }

    pub fn insert(&self, key: String, value: V) -> Option<V> {
        let idx = self.shard_index(&key);
        self.shards[idx].write().insert(key, value)
    }

    pub fn remove(&self, key: &str) -> Option<V> {
        let idx = self.shard_index(key);
        self.shards[idx].write().remove(key)
    }

    pub fn get_cloned(&self, key: &str) -> Option<V>
    where
        V: Clone,
    {
        let idx = self.shard_index(key);
        self.shards[idx].read().get(key).cloned()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        let idx = self.shard_index(key);
        self.shards[idx].read().contains_key(key)
    }

    pub fn compute<F, R>(&self, key: &str, mut f: F) -> R
    where
        F: FnMut(&mut HashMap<String, V>) -> R,
    {
        let idx = self.shard_index(key);
        let mut guard = self.shards[idx].write();
        f(&mut guard)
    }

    pub fn with_shard<F, R>(&self, key: &str, f: F) -> R
    where
        F: FnOnce(&HashMap<String, V>) -> R,
    {
        let idx = self.shard_index(key);
        let guard = self.shards[idx].read();
        f(&*guard)
    }

    pub fn with_shard_mut<F, R>(&self, key: &str, f: F) -> R
    where
        F: FnOnce(&mut HashMap<String, V>) -> R,
    {
        let idx = self.shard_index(key);
        let mut guard = self.shards[idx].write();
        f(&mut *guard)
    }

    pub fn clear(&self) {
        for s in &self.shards {
            s.write().clear();
        }
    }

    pub fn len(&self) -> usize {
        self.shards.iter().map(|s| s.read().len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.shards.iter().all(|s| s.read().is_empty())
    }

    pub fn values_snapshot(&self) -> Vec<V>
    where
        V: Clone,
    {
        let mut out = Vec::new();
        for s in &self.shards {
            let g = s.read();
            out.extend(g.values().cloned());
        }
        out
    }

    pub fn entries_snapshot(&self) -> Vec<(String, V)>
    where
        V: Clone,
    {
        let mut out = Vec::new();
        for s in &self.shards {
            let g = s.read();
            out.extend(g.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        out
    }

    pub fn retain<F>(&self, mut f: F) -> usize
    where
        F: FnMut(&str, &V) -> bool,
    {
        let mut removed = 0usize;
        for s in &self.shards {
            let mut g = s.write();
            let before = g.len();
            g.retain(|k, v| f(k, v));
            removed += before - g.len();
        }
        removed
    }

    pub fn shards_read(&self) -> Vec<RwLockReadGuard<'_, HashMap<String, V>>> {
        self.shards.iter().map(|s| s.read()).collect()
    }

    pub fn shards_write(&self) -> Vec<RwLockWriteGuard<'_, HashMap<String, V>>> {
        self.shards.iter().map(|s| s.write()).collect()
    }
}

impl<V> Default for ShardedMap<V> {
    fn default() -> Self {
        Self::new()
    }
}
