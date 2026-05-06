// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Bounded LRU cache for inline-completion suggestions.
//!
//! Cache key = hash(prefix tail 256B + suffix head 128B + language).
//! Entries carry their own TTL so replays older than the ceiling are
//! treated as misses.  The cache is `Arc + Mutex` friendly so the
//! registry can share it across threads.

use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use super::traits::{Language, Suggestion};

pub const DEFAULT_CAPACITY: usize = 512;

pub const DEFAULT_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey(u64);

impl CacheKey {
    pub fn from_context(prefix: &str, suffix: &str, lang: Language) -> Self {
        let p_tail = tail(prefix, 256);
        let s_head = head(suffix, 128);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        p_tail.hash(&mut hasher);
        s_head.hash(&mut hasher);
        lang.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Debug, Clone)]
struct Entry {
    suggestion: Suggestion,
    stored_at: Instant,
}

#[derive(Debug)]
pub struct CompletionCache {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    map: HashMap<CacheKey, Entry>,
    order: VecDeque<CacheKey>,
    capacity: usize,
    ttl: Duration,
    hits: u64,
    misses: u64,
}

impl CompletionCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
                capacity,
                ttl,
                hits: 0,
                misses: 0,
            }),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_CAPACITY, DEFAULT_TTL)
    }

    pub fn get(&self, key: CacheKey) -> Option<Suggestion> {
        let mut inner = self.inner.lock();
        let now = Instant::now();
        if let Some(entry) = inner.map.get(&key) {
            if now.duration_since(entry.stored_at) < inner.ttl {
                let s = entry.suggestion.clone();
                inner.hits += 1;

                inner.order.retain(|k| *k != key);
                inner.order.push_back(key);
                return Some(s);
            }
        }
        inner.misses += 1;
        None
    }

    pub fn put(&self, key: CacheKey, suggestion: Suggestion) {
        let mut inner = self.inner.lock();
        if inner.map.contains_key(&key) {
            inner.order.retain(|k| *k != key);
        } else if inner.map.len() >= inner.capacity {
            if let Some(evict) = inner.order.pop_front() {
                inner.map.remove(&evict);
            }
        }
        inner.map.insert(
            key,
            Entry {
                suggestion,
                stored_at: Instant::now(),
            },
        );
        inner.order.push_back(key);
    }

    pub fn hit_ratio(&self) -> f64 {
        let inner = self.inner.lock();
        let total = inner.hits + inner.misses;
        if total == 0 {
            0.0
        } else {
            inner.hits as f64 / total as f64
        }
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.map.clear();
        inner.order.clear();
    }
}

fn tail(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let mut idx = s.len() - n;
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    &s[idx..]
}

fn head(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let mut idx = n;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}
