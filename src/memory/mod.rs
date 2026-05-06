// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod ann;
pub mod audit;
pub mod backend;
pub mod blackboard;
pub mod chunker;
pub mod cli;
pub mod conflict;
pub mod consolidation;
pub mod decay;
pub mod embeddings;
pub mod gc;
#[cfg(feature = "vector-index-hnsw")]
pub mod hnsw;
pub mod hygiene;
pub mod importance;
pub mod ivf_index;
pub mod knowledge_graph;
pub mod lucid;
pub mod markdown;
pub mod none;
pub mod policy;
pub mod qdrant;
pub mod response_cache;
pub mod retrieval;
pub mod sharded_index;
pub mod sharded_map;
pub mod snapshot;
pub mod sqlite;
pub mod sqlite_vec_ext;
pub mod traits;
pub mod vector;
pub mod vector_index;

#[allow(unused_imports)]
pub use audit::AuditedMemory;
#[allow(unused_imports)]
pub use backend::{
    MemoryBackendKind, MemoryBackendProfile, classify_memory_backend, default_memory_backend_key,
    memory_backend_profile, selectable_memory_backends,
};
pub use lucid::LucidMemory;
pub use markdown::MarkdownMemory;
pub use none::NoneMemory;
#[allow(unused_imports)]
pub use policy::PolicyEnforcer;
pub use qdrant::QdrantMemory;
pub use response_cache::ResponseCache;
#[allow(unused_imports)]
pub use retrieval::{RetrievalConfig, RetrievalPipeline};
pub use sqlite::SqliteMemory;
pub use traits::Memory;
#[allow(unused_imports)]
pub use traits::{ExportFilter, MemoryCategory, MemoryEntry, ProceduralMessage};

use crate::config::{EmbeddingRouteConfig, MemoryConfig, StorageProviderConfig};
use anyhow::Context;
use std::path::Path;
use std::sync::Arc;

fn create_memory_with_builders<F>(
    backend_name: &str,
    workspace_dir: &Path,
    mut sqlite_builder: F,
    unknown_context: &str,
) -> anyhow::Result<Box<dyn Memory>>
where
    F: FnMut() -> anyhow::Result<SqliteMemory>,
{
    match classify_memory_backend(backend_name) {
        MemoryBackendKind::Sqlite => Ok(Box::new(sqlite_builder()?)),
        MemoryBackendKind::Lucid => {
            let local = sqlite_builder()?;
            Ok(Box::new(LucidMemory::new(workspace_dir, local)))
        }
        MemoryBackendKind::Qdrant | MemoryBackendKind::Markdown => {
            Ok(Box::new(MarkdownMemory::new(workspace_dir)))
        }
        MemoryBackendKind::None => Ok(Box::new(NoneMemory::new())),
        MemoryBackendKind::Unknown => {
            tracing::warn!(
                "Unknown memory backend '{backend_name}'{unknown_context}, falling back to markdown"
            );
            Ok(Box::new(MarkdownMemory::new(workspace_dir)))
        }
    }
}

pub fn effective_memory_backend_name(
    memory_backend: &str,
    storage_provider: Option<&StorageProviderConfig>,
) -> String {
    if let Some(override_provider) = storage_provider
        .map(|cfg| cfg.provider.trim())
        .filter(|provider| !provider.is_empty())
    {
        return override_provider.to_ascii_lowercase();
    }

    memory_backend.trim().to_ascii_lowercase()
}

pub fn is_assistant_autosave_key(key: &str) -> bool {
    let normalized = key.trim().to_ascii_lowercase();
    normalized == "assistant_resp" || normalized.starts_with("assistant_resp_")
}

pub fn should_skip_autosave_content(content: &str) -> bool {
    let normalized = content.trim();
    if normalized.is_empty() {
        return true;
    }

    let lowered = normalized.to_ascii_lowercase();
    lowered.starts_with("[cron:")
        || lowered.starts_with("[heartbeat task")
        || lowered.starts_with("[distilled_")
        || lowered.contains("distilled_index_sig:")
}

#[derive(Clone, PartialEq, Eq)]
struct ResolvedEmbeddingConfig {
    provider: String,
    model: String,
    dimensions: usize,
    api_key: Option<String>,
}

impl std::fmt::Debug for ResolvedEmbeddingConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedEmbeddingConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("dimensions", &self.dimensions)
            .finish_non_exhaustive()
    }
}

fn embedding_provider_env_key(provider: &str) -> Option<String> {
    let env_var = match provider.trim() {
        "openai" => "OPENAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "cohere" => "COHERE_API_KEY",
        _ => return None,
    };
    std::env::var(env_var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn resolve_embedding_config(
    config: &MemoryConfig,
    embedding_routes: &[EmbeddingRouteConfig],
    api_key: Option<&str>,
) -> ResolvedEmbeddingConfig {
    let caller_api_key = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let fallback_api_key =
        embedding_provider_env_key(config.embedding_provider.trim()).or(caller_api_key);
    let fallback = ResolvedEmbeddingConfig {
        provider: config.embedding_provider.trim().to_string(),
        model: config.embedding_model.trim().to_string(),
        dimensions: config.embedding_dimensions,
        api_key: fallback_api_key.clone(),
    };

    let Some(hint) = config
        .embedding_model
        .strip_prefix("hint:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return fallback;
    };

    let Some(route) = embedding_routes
        .iter()
        .find(|route| route.hint.trim() == hint)
    else {
        tracing::warn!(
            hint,
            "Unknown embedding route hint; falling back to [memory] embedding settings"
        );
        return fallback;
    };

    let provider = route.provider.trim();
    let model = route.model.trim();
    let dimensions = route.dimensions.unwrap_or(config.embedding_dimensions);
    if provider.is_empty() || model.is_empty() || dimensions == 0 {
        tracing::warn!(
            hint,
            "Invalid embedding route configuration; falling back to [memory] embedding settings"
        );
        return fallback;
    }

    let routed_api_key = route
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value: &&str| !value.is_empty())
        .map(|value| value.to_string());

    ResolvedEmbeddingConfig {
        provider: provider.to_string(),
        model: model.to_string(),
        dimensions,
        api_key: routed_api_key.or(fallback_api_key),
    }
}

pub fn create_memory(
    config: &MemoryConfig,
    workspace_dir: &Path,
    api_key: Option<&str>,
) -> anyhow::Result<Box<dyn Memory>> {
    create_memory_with_storage_and_routes(config, &[], None, workspace_dir, api_key)
}

pub fn create_memory_with_storage(
    config: &MemoryConfig,
    storage_provider: Option<&StorageProviderConfig>,
    workspace_dir: &Path,
    api_key: Option<&str>,
) -> anyhow::Result<Box<dyn Memory>> {
    create_memory_with_storage_and_routes(config, &[], storage_provider, workspace_dir, api_key)
}

pub fn create_memory_with_storage_and_routes(
    config: &MemoryConfig,
    embedding_routes: &[EmbeddingRouteConfig],
    storage_provider: Option<&StorageProviderConfig>,
    workspace_dir: &Path,
    api_key: Option<&str>,
) -> anyhow::Result<Box<dyn Memory>> {
    let backend_name = effective_memory_backend_name(&config.backend, storage_provider);
    let backend_kind = classify_memory_backend(&backend_name);
    let resolved_embedding = resolve_embedding_config(config, embedding_routes, api_key);

    if let Err(e) = hygiene::run_if_due(config, workspace_dir) {
        tracing::warn!("memory hygiene skipped: {e}");
    }

    if config.snapshot_enabled
        && config.snapshot_on_hygiene
        && matches!(
            backend_kind,
            MemoryBackendKind::Sqlite | MemoryBackendKind::Lucid
        )
    {
        if let Err(e) = snapshot::export_snapshot(workspace_dir) {
            tracing::warn!("memory snapshot skipped: {e}");
        }
    }

    if config.auto_hydrate
        && matches!(
            backend_kind,
            MemoryBackendKind::Sqlite | MemoryBackendKind::Lucid
        )
        && snapshot::should_hydrate(workspace_dir)
    {
        tracing::info!("🧬 Cold boot detected — hydrating from MEMORY_SNAPSHOT.md");
        match snapshot::hydrate_from_snapshot(workspace_dir) {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("🧬 Hydrated {count} core memories from snapshot");
                }
            }
            Err(e) => {
                tracing::warn!("memory hydration failed: {e}");
            }
        }
    }

    fn build_sqlite_memory(
        config: &MemoryConfig,
        workspace_dir: &Path,
        resolved_embedding: &ResolvedEmbeddingConfig,
    ) -> anyhow::Result<SqliteMemory> {
        let embedder: Arc<dyn embeddings::EmbeddingProvider> =
            Arc::from(embeddings::create_embedding_provider(
                &resolved_embedding.provider,
                resolved_embedding.api_key.as_deref(),
                &resolved_embedding.model,
                resolved_embedding.dimensions,
            ));

        #[allow(clippy::cast_possible_truncation)]
        let mem = SqliteMemory::with_embedder(
            workspace_dir,
            embedder,
            config.vector_weight as f32,
            config.keyword_weight as f32,
            config.embedding_cache_size,
            config.sqlite_open_timeout_secs,
            config.search_mode.clone(),
        )?;
        Ok(mem)
    }

    if matches!(backend_kind, MemoryBackendKind::Qdrant) {
        let url = config
            .qdrant
            .url
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("QDRANT_URL").ok())
            .filter(|s| !s.trim().is_empty())
            .context(
                "Qdrant memory backend requires url in [memory.qdrant] or QDRANT_URL env var",
            )?;
        let collection = std::env::var("QDRANT_COLLECTION")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| config.qdrant.collection.clone());
        let qdrant_api_key = config
            .qdrant
            .api_key
            .clone()
            .or_else(|| std::env::var("QDRANT_API_KEY").ok())
            .filter(|s| !s.trim().is_empty());
        let embedder: Arc<dyn embeddings::EmbeddingProvider> =
            Arc::from(embeddings::create_embedding_provider(
                &resolved_embedding.provider,
                resolved_embedding.api_key.as_deref(),
                &resolved_embedding.model,
                resolved_embedding.dimensions,
            ));
        tracing::info!(
            "📦 Qdrant memory backend configured (url: {}, collection: {})",
            url,
            collection
        );
        return Ok(Box::new(QdrantMemory::new_lazy(
            &url,
            &collection,
            qdrant_api_key,
            embedder,
        )));
    }

    let mem = create_memory_with_builders(
        &backend_name,
        workspace_dir,
        || build_sqlite_memory(config, workspace_dir, &resolved_embedding),
        "",
    )?;

    if config.audit_enabled {
        tracing::info!("Memory audit logging enabled");

        if matches!(backend_kind, MemoryBackendKind::Sqlite) {
            match build_sqlite_memory(config, workspace_dir, &resolved_embedding) {
                Ok(sqlite_mem) => match audit::AuditedMemory::new(sqlite_mem, workspace_dir) {
                    Ok(audited) => return Ok(Box::new(audited)),
                    Err(e) => tracing::warn!("Failed to enable memory audit: {e}"),
                },
                Err(e) => tracing::warn!("Failed to build audited memory: {e}"),
            }
        } else {
            tracing::info!("Audit logging is only supported with the sqlite backend");
        }
    }

    Ok(mem)
}

pub fn create_memory_for_migration(
    backend: &str,
    workspace_dir: &Path,
) -> anyhow::Result<Box<dyn Memory>> {
    if matches!(classify_memory_backend(backend), MemoryBackendKind::None) {
        anyhow::bail!(
            "memory backend 'none' disables persistence; choose sqlite, lucid, or markdown before migration"
        );
    }

    create_memory_with_builders(
        backend,
        workspace_dir,
        || SqliteMemory::new(workspace_dir),
        " during migration",
    )
}

pub fn create_response_cache(config: &MemoryConfig, workspace_dir: &Path) -> Option<ResponseCache> {
    if !config.response_cache_enabled {
        return None;
    }

    match ResponseCache::new(
        workspace_dir,
        config.response_cache_ttl_minutes,
        config.response_cache_max_entries,
    ) {
        Ok(cache) => {
            tracing::info!(
                "💾 Response cache enabled (TTL: {}min, max: {} entries)",
                config.response_cache_ttl_minutes,
                config.response_cache_max_entries
            );
            Some(cache)
        }
        Err(e) => {
            tracing::warn!("Response cache disabled due to error: {e}");
            None
        }
    }
}
