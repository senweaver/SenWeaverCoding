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
pub mod sharded;
pub mod snapshot;
pub mod sqlite;
pub mod traits;
pub mod vector;
pub use audit::AuditedMemory;
pub use backend::{
    MemoryBackendKind, MemoryBackendProfile, classify_memory_backend, default_memory_backend_key,
    memory_backend_profile, selectable_memory_backends,
};
pub use lucid::LucidMemory;
pub use markdown::MarkdownMemory;
pub use none::NoneMemory;
pub use policy::PolicyEnforcer;
pub use qdrant::QdrantMemory;
pub use response_cache::ResponseCache;
pub use retrieval::{RetrievalConfig, RetrievalPipeline};
pub use sqlite::SqliteMemory;
pub use traits::Memory;
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
        MemoryBackendKind::Markdown => Ok(Box::new(MarkdownMemory::new(workspace_dir))),
        MemoryBackendKind::Qdrant => Err(anyhow::anyhow!(
            "Qdrant backend must be constructed via the dedicated path in create_memory_with_storage_and_routes, not the sqlite/lucid/markdown fallback helper{unknown_context}"
        )),
        MemoryBackendKind::None => Ok(Box::new(NoneMemory::new())),
        MemoryBackendKind::Unknown => Err(anyhow::anyhow!(
            "Unknown memory backend '{backend_name}'{unknown_context}. Configure [memory].backend to one of: sqlite, lucid, markdown, qdrant, none.",
        )),
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EmbeddingApiKeySource {

    Route(String),

    Env(String),

    Caller,

    None,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EmbeddingConfigSource {

    Toml,

    Route(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct EmbeddingResolution {
    pub provider: String,
    pub model: String,
    pub dimensions: usize,
    pub api_key: Option<String>,
    pub config_source: EmbeddingConfigSource,
    pub api_key_source: EmbeddingApiKeySource,
}

impl std::fmt::Debug for EmbeddingResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingResolution")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("dimensions", &self.dimensions)
            .field("config_source", &self.config_source)
            .field("api_key_source", &self.api_key_source)
            .field("api_key_present", &self.api_key.is_some())
            .finish_non_exhaustive()
    }
}

type ResolvedEmbeddingConfig = EmbeddingResolution;

fn embedding_provider_env_key(provider: &str) -> Option<(String, String)> {
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
        .map(|v| (env_var.to_string(), v))
}

fn resolve_embedding_config(
    config: &MemoryConfig,
    embedding_routes: &[EmbeddingRouteConfig],
    api_key: Option<&str>,
) -> EmbeddingResolution {
    let caller_api_key = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let env_pair = embedding_provider_env_key(config.embedding_provider.trim());
    let (fallback_api_key, fallback_api_key_source) = match (env_pair.as_ref(), caller_api_key.as_ref()) {
        (Some((var, val)), _) => (Some(val.clone()), EmbeddingApiKeySource::Env(var.clone())),
        (None, Some(val)) => (Some(val.clone()), EmbeddingApiKeySource::Caller),
        (None, None) => (None, EmbeddingApiKeySource::None),
    };

    let fallback = EmbeddingResolution {
        provider: config.embedding_provider.trim().to_string(),
        model: config.embedding_model.trim().to_string(),
        dimensions: config.embedding_dimensions,
        api_key: fallback_api_key.clone(),
        config_source: EmbeddingConfigSource::Toml,
        api_key_source: fallback_api_key_source.clone(),
    };

    let route_hint = config
        .embedding_model
        .strip_prefix("route:")
        .or_else(|| {
            config
                .embedding_model
                .strip_prefix("hint:")
                .inspect(|_| {
                    tracing::warn!(
                        deprecated = "hint:",
                        replacement = "route:",
                        "embedding_model uses deprecated `hint:` prefix; switch to `route:` (hint: still accepted for now)"
                    );
                })
        })
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(route_key) = route_hint else {
        log_resolution(&fallback);
        return fallback;
    };

    let Some(route) = embedding_routes
        .iter()
        .find(|route| route.hint.trim() == route_key)
    else {
        tracing::warn!(
            route = route_key,
            "Unknown embedding route; falling back to [memory] embedding settings"
        );
        log_resolution(&fallback);
        return fallback;
    };

    let provider = route.provider.trim();
    let model = route.model.trim();
    let dimensions = route.dimensions.unwrap_or(config.embedding_dimensions);
    if provider.is_empty() || model.is_empty() || dimensions == 0 {
        tracing::warn!(
            route = route_key,
            "Invalid embedding route configuration; falling back to [memory] embedding settings"
        );
        log_resolution(&fallback);
        return fallback;
    }

    let routed_api_key = route
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value: &&str| !value.is_empty())
        .map(|value| value.to_string());

    let (api_key, api_key_source) = match (routed_api_key, fallback_api_key) {
        (Some(k), _) => (Some(k), EmbeddingApiKeySource::Route(route_key.to_string())),
        (None, Some(k)) => (Some(k), fallback_api_key_source),
        (None, None) => (None, EmbeddingApiKeySource::None),
    };

    let resolved = EmbeddingResolution {
        provider: provider.to_string(),
        model: model.to_string(),
        dimensions,
        api_key,
        config_source: EmbeddingConfigSource::Route(route_key.to_string()),
        api_key_source,
    };
    log_resolution(&resolved);
    resolved
}

fn log_resolution(r: &EmbeddingResolution) {
    tracing::info!(
        provider = %r.provider,
        model = %r.model,
        dimensions = r.dimensions,
        config_source = ?r.config_source,
        api_key_source = ?r.api_key_source,
        "embedding configuration resolved",
    );
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

#[allow(clippy::too_many_arguments)]
pub async fn create_memory_with_storage_and_routes_async(
    config: MemoryConfig,
    embedding_routes: Vec<EmbeddingRouteConfig>,
    storage_provider: Option<StorageProviderConfig>,
    workspace_dir: std::path::PathBuf,
    api_key: Option<String>,
) -> anyhow::Result<Box<dyn Memory>> {
    tokio::task::spawn_blocking(move || {
        create_memory_with_storage_and_routes(
            &config,
            &embedding_routes,
            storage_provider.as_ref(),
            &workspace_dir,
            api_key.as_deref(),
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("memory initialization task failed: {e}"))?
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
        tracing::info!("🧬 Cold boot detected  -  hydrating from MEMORY_SNAPSHOT.md");
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
        )?));
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
    match classify_memory_backend(backend) {
        MemoryBackendKind::None => anyhow::bail!(
            "memory backend 'none' disables persistence; choose sqlite, lucid, or markdown before migration"
        ),
        MemoryBackendKind::Qdrant => anyhow::bail!(
            "qdrant backend cannot be used as a migration source/target; use sqlite, lucid, or markdown for migration"
        ),
        MemoryBackendKind::Unknown => anyhow::bail!(
            "unknown memory backend '{backend}' during migration; choose sqlite, lucid, or markdown"
        ),
        MemoryBackendKind::Sqlite | MemoryBackendKind::Lucid | MemoryBackendKind::Markdown => {
            create_memory_with_builders(
                backend,
                workspace_dir,
                || SqliteMemory::new(workspace_dir),
                " during migration",
            )
        }
    }
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
