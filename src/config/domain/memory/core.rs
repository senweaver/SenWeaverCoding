// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QdrantConfig {

    #[serde(default)]
    pub url: Option<String>,

    #[serde(default = "default_qdrant_collection")]
    pub collection: String,

    #[serde(default)]
    pub api_key: Option<String>,
}

pub(crate) fn default_qdrant_collection() -> String {
    "sen_memories".into()
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            url: None,
            collection: default_qdrant_collection(),
            api_key: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {

    Bm25,

    Embedding,

    #[default]
    Hybrid,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct MemoryPolicyConfig {

    #[serde(default)]
    pub max_entries_per_namespace: usize,

    #[serde(default)]
    pub max_entries_per_category: usize,

    #[serde(default)]
    pub retention_days_by_category: HashMap<String, u32>,

    #[serde(default)]
    pub read_only_namespaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct MemoryConfig {

    pub backend: String,

    pub auto_save: bool,

    #[serde(default = "default_hygiene_enabled")]
    pub hygiene_enabled: bool,

    #[serde(default = "default_archive_after_days")]
    pub archive_after_days: u32,

    #[serde(default = "default_purge_after_days")]
    pub purge_after_days: u32,

    #[serde(default = "default_conversation_retention_days")]
    pub conversation_retention_days: u32,

    #[serde(default = "default_embedding_provider")]
    pub embedding_provider: String,

    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,

    #[serde(default = "default_embedding_dims")]
    pub embedding_dimensions: usize,

    #[serde(default = "default_vector_weight")]
    pub vector_weight: f64,

    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f64,

    #[serde(default)]
    pub search_mode: SearchMode,

    #[serde(default)]
    pub vector_backend: Option<crate::memory::vector::index::VectorBackend>,

    #[serde(default = "default_min_relevance_score")]
    pub min_relevance_score: f64,

    #[serde(default = "default_cache_size")]
    pub embedding_cache_size: usize,

    #[serde(default = "default_chunk_size")]
    pub chunk_max_tokens: usize,

    #[serde(default)]
    pub response_cache_enabled: bool,
    #[serde(default = "default_response_cache_ttl")]
    pub response_cache_ttl_minutes: u32,
    #[serde(default = "default_response_cache_max")]
    pub response_cache_max_entries: usize,
    #[serde(default = "default_response_cache_hot_entries")]
    pub response_cache_hot_entries: usize,

    #[serde(default)]
    pub snapshot_enabled: bool,
    #[serde(default)]
    pub snapshot_on_hygiene: bool,
    #[serde(default = "default_true_bool")]
    pub auto_hydrate: bool,

    #[serde(default = "default_retrieval_stages")]
    pub retrieval_stages: Vec<String>,
    #[serde(default)]
    pub rerank_enabled: bool,
    #[serde(default = "default_rerank_threshold")]
    pub rerank_threshold: usize,
    #[serde(default = "default_fts_early_return_score")]
    pub fts_early_return_score: f64,

    #[serde(default = "default_namespace")]
    pub default_namespace: String,

    #[serde(default = "default_conflict_threshold")]
    pub conflict_threshold: f64,

    #[serde(default)]
    pub audit_enabled: bool,
    #[serde(default = "default_audit_retention_days")]
    pub audit_retention_days: u32,

    #[serde(default)]
    pub policy: MemoryPolicyConfig,

    #[serde(default)]
    pub sqlite_open_timeout_secs: Option<u64>,

    #[serde(default)]
    pub qdrant: QdrantConfig,
}

pub(crate) fn default_retrieval_stages() -> Vec<String> {
    vec!["cache".into(), "fts".into(), "vector".into()]
}
pub(crate) fn default_rerank_threshold() -> usize {
    5
}
pub(crate) fn default_fts_early_return_score() -> f64 {
    0.85
}
pub(crate) fn default_namespace() -> String {
    "default".into()
}
pub(crate) fn default_conflict_threshold() -> f64 {
    0.85
}
pub(crate) fn default_audit_retention_days() -> u32 {
    30
}
pub(crate) fn default_embedding_provider() -> String {
    "none".into()
}
pub(crate) fn default_hygiene_enabled() -> bool {
    true
}
pub(crate) fn default_archive_after_days() -> u32 {
    7
}
pub(crate) fn default_purge_after_days() -> u32 {
    30
}
pub(crate) fn default_conversation_retention_days() -> u32 {
    30
}
pub(crate) fn default_embedding_model() -> String {
    "text-embedding-3-small".into()
}
pub(crate) fn default_embedding_dims() -> usize {
    1536
}
pub(crate) fn default_vector_weight() -> f64 {
    0.7
}
pub(crate) fn default_keyword_weight() -> f64 {
    0.3
}
pub(crate) fn default_min_relevance_score() -> f64 {
    0.4
}
pub(crate) fn default_cache_size() -> usize {
    10_000
}
pub(crate) fn default_chunk_size() -> usize {
    512
}
pub(crate) fn default_response_cache_ttl() -> u32 {
    60
}
pub(crate) fn default_response_cache_max() -> usize {
    5_000
}
pub(crate) fn default_response_cache_hot_entries() -> usize {
    256
}
pub(crate) fn default_true_bool() -> bool {
    true
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: "sqlite".into(),
            auto_save: true,
            hygiene_enabled: default_hygiene_enabled(),
            archive_after_days: default_archive_after_days(),
            purge_after_days: default_purge_after_days(),
            conversation_retention_days: default_conversation_retention_days(),
            embedding_provider: default_embedding_provider(),
            embedding_model: default_embedding_model(),
            embedding_dimensions: default_embedding_dims(),
            vector_weight: default_vector_weight(),
            keyword_weight: default_keyword_weight(),
            search_mode: SearchMode::default(),
            vector_backend: None,
            min_relevance_score: default_min_relevance_score(),
            embedding_cache_size: default_cache_size(),
            chunk_max_tokens: default_chunk_size(),
            response_cache_enabled: false,
            response_cache_ttl_minutes: default_response_cache_ttl(),
            response_cache_max_entries: default_response_cache_max(),
            response_cache_hot_entries: default_response_cache_hot_entries(),
            snapshot_enabled: false,
            snapshot_on_hygiene: false,
            auto_hydrate: true,
            retrieval_stages: default_retrieval_stages(),
            rerank_enabled: false,
            rerank_threshold: default_rerank_threshold(),
            fts_early_return_score: default_fts_early_return_score(),
            default_namespace: default_namespace(),
            conflict_threshold: default_conflict_threshold(),
            audit_enabled: false,
            audit_retention_days: default_audit_retention_days(),
            policy: MemoryPolicyConfig::default(),
            sqlite_open_timeout_secs: None,
            qdrant: QdrantConfig::default(),
        }
    }
}

impl MemoryConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let valid_backends = ["sqlite", "lucid", "qdrant", "markdown", "none"];
        if !valid_backends.contains(&self.backend.as_str()) {
            errors.push(format!(
                "memory.backend '{}' is not one of {:?}",
                self.backend, valid_backends
            ));
        }
        if self.backend == "qdrant" {
            let effective_url = self
                .qdrant
                .url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    std::env::var("QDRANT_URL")
                        .ok()
                        .map(|v| v.trim().to_string())
                        .filter(|s| !s.is_empty())
                });
            match effective_url {
                None => errors.push(
                    "memory.backend=qdrant requires memory.qdrant.url or QDRANT_URL env".into(),
                ),
                Some(url) => match reqwest::Url::parse(&url) {
                    Ok(parsed) => {
                        if !matches!(parsed.scheme(), "http" | "https") {
                            errors.push(format!(
                                "memory.qdrant.url scheme '{}' is unsupported; expected http or https",
                                parsed.scheme()
                            ));
                        }
                        if parsed.host_str().map(str::is_empty).unwrap_or(true) {
                            errors.push(format!(
                                "memory.qdrant.url '{url}' is missing a host component"
                            ));
                        }
                    }
                    Err(e) => errors.push(format!(
                        "memory.qdrant.url '{url}' failed to parse: {e} (expected http(s)://host[:port])"
                    )),
                },
            }
        }
        if self.vector_weight < 0.0 || self.vector_weight > 1.0 {
            errors.push(format!(
                "memory.vector_weight must be in [0, 1], got {}",
                self.vector_weight
            ));
        }
        if self.keyword_weight < 0.0 || self.keyword_weight > 1.0 {
            errors.push(format!(
                "memory.keyword_weight must be in [0, 1], got {}",
                self.keyword_weight
            ));
        }
        if self.min_relevance_score < 0.0 || self.min_relevance_score > 1.0 {
            errors.push(format!(
                "memory.min_relevance_score must be in [0, 1], got {}",
                self.min_relevance_score
            ));
        }
        if self.embedding_dimensions == 0 {
            errors.push("memory.embedding_dimensions must be > 0".into());
        }
        errors
    }
}
