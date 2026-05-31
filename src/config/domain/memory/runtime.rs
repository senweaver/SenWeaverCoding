// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRuntimeExtras {

    #[serde(default = "default_ephemeral_retention_secs")]
    pub ephemeral_retention_secs: u64,

    #[serde(default = "default_session_retention_secs")]
    pub session_retention_secs: u64,

    #[serde(default = "default_gc_interval_secs")]
    pub gc_interval_secs: u64,

    #[serde(default = "default_embedding_cache_max")]
    pub embedding_cache_max: usize,

    #[serde(default = "default_read_pool_size")]
    pub read_pool_size: usize,

    #[serde(default = "default_vector_backend")]
    pub vector_backend: String,

    #[serde(default = "default_slow_search_warn_at")]
    pub slow_search_warn_at: u64,

    #[serde(default = "default_similarity_floor")]
    pub similarity_floor: f32,
}

fn default_ephemeral_retention_secs() -> u64 {
    3600
}
fn default_session_retention_secs() -> u64 {
    86_400 * 7
}
fn default_gc_interval_secs() -> u64 {
    300
}
fn default_embedding_cache_max() -> usize {
    10_000
}
fn default_read_pool_size() -> usize {
    4
}
fn default_vector_backend() -> String {
    "linear".into()
}
fn default_slow_search_warn_at() -> u64 {
    50_000
}
fn default_similarity_floor() -> f32 {
    0.0
}

impl Default for MemoryRuntimeExtras {
    fn default() -> Self {
        Self {
            ephemeral_retention_secs: default_ephemeral_retention_secs(),
            session_retention_secs: default_session_retention_secs(),
            gc_interval_secs: default_gc_interval_secs(),
            embedding_cache_max: default_embedding_cache_max(),
            read_pool_size: default_read_pool_size(),
            vector_backend: default_vector_backend(),
            slow_search_warn_at: default_slow_search_warn_at(),
            similarity_floor: default_similarity_floor(),
        }
    }
}

impl MemoryRuntimeExtras {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.session_retention_secs < self.ephemeral_retention_secs {
            errors.push(
                "memory_runtime.session_retention_secs must be >= ephemeral_retention_secs".into(),
            );
        }
        if self.gc_interval_secs == 0 {
            errors.push(
                "memory_runtime.gc_interval_secs must be > 0 (use a large value to effectively disable)"
                    .into(),
            );
        }
        if self.embedding_cache_max == 0 {
            errors.push("memory_runtime.embedding_cache_max must be > 0".into());
        }
        if self.read_pool_size == 0 {
            errors.push("memory_runtime.read_pool_size must be >= 1".into());
        }
        if self.read_pool_size > 32 {
            errors.push(
                "memory_runtime.read_pool_size > 32 is unusual  -  likely misconfigured".into(),
            );
        }
        let allowed = ["linear", "sqlite_persistent"];
        if !allowed.contains(&self.vector_backend.as_str()) {
            errors.push(format!(
                "memory_runtime.vector_backend must be one of {allowed:?}, got '{}'",
                self.vector_backend
            ));
        }
        if !(0.0..=1.0).contains(&self.similarity_floor) {
            errors.push(format!(
                "memory_runtime.similarity_floor must be in [0.0, 1.0], got {}",
                self.similarity_floor
            ));
        }
        errors
    }

    pub fn gc_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.gc_interval_secs.max(1))
    }

    pub fn ephemeral_retention(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.ephemeral_retention_secs)
    }
}
