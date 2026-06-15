// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{ExportFilter, Memory, MemoryCategory, MemoryEntry, ProceduralMessage};
use async_trait::async_trait;
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

    pub rerank_enabled: bool,

    pub rerank_threshold: usize,

    pub cache_max_entries: usize,

    pub cache_ttl: Duration,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            stages: vec!["cache".into(), "fts".into(), "vector".into()],
            fts_early_return_score: 0.85,
            rerank_enabled: false,
            rerank_threshold: 5,
            cache_max_entries: 256,
            cache_ttl: Duration::from_secs(300),
        }
    }
}

impl RetrievalConfig {
    pub fn from_memory_config(config: &crate::config::MemoryConfig) -> Self {
        let defaults = Self::default();
        Self {
            stages: config.retrieval_stages.clone(),
            fts_early_return_score: config.fts_early_return_score,
            rerank_enabled: config.rerank_enabled,
            rerank_threshold: config.rerank_threshold.max(1),
            cache_max_entries: defaults.cache_max_entries,
            cache_ttl: defaults.cache_ttl,
        }
    }

    pub fn matches_direct_path(config: &crate::config::MemoryConfig) -> bool {
        let defaults = Self::default();
        !config.rerank_enabled
            && config.retrieval_stages == defaults.stages
            && (config.fts_early_return_score - defaults.fts_early_return_score).abs()
                <= f64::EPSILON
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

                    let mut results = if let Some(ns) = namespace {
                        self.memory
                            .recall_namespaced(ns, query, limit, session_id, since, until)
                            .await?
                    } else {
                        self.memory
                            .recall(query, limit, session_id, since, until)
                            .await?
                    };

                    if !results.is_empty() {
                        if self.config.rerank_enabled
                            && results.len() >= self.config.rerank_threshold
                        {
                            rerank_entries(query, &mut results);
                            tracing::debug!(
                                "retrieval pipeline: reranked {} results",
                                results.len()
                            );
                        }

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

#[allow(clippy::cast_precision_loss)]
fn rerank_entries(query: &str, entries: &mut [MemoryEntry]) {
    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() > 1)
        .collect();
    if terms.is_empty() || entries.is_empty() {
        return;
    }

    let max_base = entries
        .iter()
        .filter_map(|e| e.score)
        .fold(0.0_f64, f64::max);

    for entry in entries.iter_mut() {
        let content_lower = entry.content.to_lowercase();
        let key_lower = entry.key.to_lowercase();
        let matched = terms
            .iter()
            .filter(|t| content_lower.contains(**t) || key_lower.contains(**t))
            .count();
        let overlap = matched as f64 / terms.len() as f64;
        let base = entry.score.unwrap_or(0.0);
        let norm_base = if max_base > 0.0 {
            (base / max_base).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let importance = entry.importance.unwrap_or(0.5).clamp(0.0, 1.0);
        entry.score = Some(0.55 * norm_base + 0.35 * overlap + 0.10 * importance);
    }

    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

pub struct PipelinedMemory {
    inner: Arc<dyn Memory>,
    pipeline: RetrievalPipeline,
}

impl PipelinedMemory {
    pub fn new(inner: Arc<dyn Memory>, config: RetrievalConfig) -> Self {
        let pipeline = RetrievalPipeline::new(inner.clone(), config);
        Self { inner, pipeline }
    }

    pub fn pipeline(&self) -> &RetrievalPipeline {
        &self.pipeline
    }
}

#[async_trait]
impl Memory for PipelinedMemory {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn store(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let result = self.inner.store(key, content, category, session_id).await;
        if result.is_ok() {
            self.pipeline.invalidate_cache();
        }
        result
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        self.pipeline
            .recall(query, limit, session_id, None, since, until)
            .await
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        self.inner.get(key).await
    }

    async fn list(
        &self,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        self.inner.list(category, session_id).await
    }

    async fn forget(&self, key: &str) -> anyhow::Result<bool> {
        let result = self.inner.forget(key).await;
        if matches!(result, Ok(true)) {
            self.pipeline.invalidate_cache();
        }
        result
    }

    async fn purge_namespace(&self, namespace: &str) -> anyhow::Result<usize> {
        let result = self.inner.purge_namespace(namespace).await;
        if result.is_ok() {
            self.pipeline.invalidate_cache();
        }
        result
    }

    async fn purge_session(&self, session_id: &str) -> anyhow::Result<usize> {
        let result = self.inner.purge_session(session_id).await;
        if result.is_ok() {
            self.pipeline.invalidate_cache();
        }
        result
    }

    async fn count(&self) -> anyhow::Result<usize> {
        self.inner.count().await
    }

    async fn health_check(&self) -> bool {
        self.inner.health_check().await
    }

    async fn store_procedural(
        &self,
        messages: &[ProceduralMessage],
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.store_procedural(messages, session_id).await
    }

    async fn recall_namespaced(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        self.pipeline
            .recall(query, limit, session_id, Some(namespace), since, until)
            .await
    }

    async fn export(&self, filter: &ExportFilter) -> anyhow::Result<Vec<MemoryEntry>> {
        self.inner.export(filter).await
    }

    async fn store_with_metadata(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        namespace: Option<&str>,
        importance: Option<f64>,
    ) -> anyhow::Result<()> {
        let result = self
            .inner
            .store_with_metadata(key, content, category, session_id, namespace, importance)
            .await;
        if result.is_ok() {
            self.pipeline.invalidate_cache();
        }
        result
    }
}
