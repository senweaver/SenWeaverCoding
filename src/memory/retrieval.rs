// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Memory, MemoryEntry};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct CachedResult {
    entries: Vec<MemoryEntry>,
    created_at: Instant,
}

#[derive(Debug, Clone)]
pub struct RetrievalConfig {

    pub stages: Vec<String>,

    pub fts_early_return_score: f64,

    pub cache_max_entries: usize,

    pub cache_ttl: Duration,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            stages: vec!["cache".into(), "fts".into(), "vector".into()],
            fts_early_return_score: 0.85,
            cache_max_entries: 256,
            cache_ttl: Duration::from_secs(300),
        }
    }
}

pub struct RetrievalPipeline {
    memory: Arc<dyn Memory>,
    config: RetrievalConfig,
    hot_cache: Mutex<HashMap<String, CachedResult>>,
}

impl RetrievalPipeline {
    pub fn new(memory: Arc<dyn Memory>, config: RetrievalConfig) -> Self {
        Self {
            memory,
            config,
            hot_cache: Mutex::new(HashMap::new()),
        }
    }

    fn cache_key(
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        namespace: Option<&str>,
    ) -> String {
        format!(
            "{}:{}:{}:{}",
            query,
            limit,
            session_id.unwrap_or(""),
            namespace.unwrap_or("")
        )
    }

    fn check_cache(&self, key: &str) -> Option<Vec<MemoryEntry>> {
        let cache = self.hot_cache.lock();
        if let Some(cached) = cache.get(key) {
            if cached.created_at.elapsed() < self.config.cache_ttl {
                return Some(cached.entries.clone());
            }
        }
        None
    }

    fn store_in_cache(&self, key: String, entries: Vec<MemoryEntry>) {
        let mut cache = self.hot_cache.lock();

        if cache.len() >= self.config.cache_max_entries {
            let oldest_key = cache
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                cache.remove(&k);
            }
        }

        cache.insert(
            key,
            CachedResult {
                entries,
                created_at: Instant::now(),
            },
        );
    }

    pub async fn recall(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        namespace: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let ck = Self::cache_key(query, limit, session_id, namespace);

        for stage in &self.config.stages {
            match stage.as_str() {
                "cache" => {
                    if let Some(cached) = self.check_cache(&ck) {
                        tracing::debug!("retrieval pipeline: cache hit for '{query}'");
                        return Ok(cached);
                    }
                }
                "fts" | "vector" => {

                    let results = if let Some(ns) = namespace {
                        self.memory
                            .recall_namespaced(ns, query, limit, session_id, since, until)
                            .await?
                    } else {
                        self.memory
                            .recall(query, limit, session_id, since, until)
                            .await?
                    };

                    if !results.is_empty() {

                        if stage == "fts" {
                            if let Some(top_score) = results.first().and_then(|e| e.score) {
                                if top_score >= self.config.fts_early_return_score {
                                    tracing::debug!(
                                        "retrieval pipeline: FTS early return (score={top_score:.3})"
                                    );
                                    self.store_in_cache(ck, results.clone());
                                    return Ok(results);
                                }
                            }
                        }

                        self.store_in_cache(ck, results.clone());
                        return Ok(results);
                    }
                }
                other => {
                    tracing::warn!("retrieval pipeline: unknown stage '{other}', skipping");
                }
            }
        }

        Ok(Vec::new())
    }

    pub fn invalidate_cache(&self) {
        self.hot_cache.lock().clear();
    }

    pub fn cache_size(&self) -> usize {
        self.hot_cache.lock().len()
    }
}
