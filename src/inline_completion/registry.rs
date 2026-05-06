// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Registry that wires throttling, caching, and provider selection.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

use super::cache::{CacheKey, CompletionCache};
use super::stats::{AcceptanceEvent, global_stats};
use super::throttle::{Throttler, ThrottlerDecision};
use super::traits::{
    InlineCompletionError, InlineCompletionProvider, InlineCompletionRequest,
    InlineCompletionResponse, Language,
};

pub type RegistryHandle = Arc<InlineCompletionRegistry>;

pub struct InlineCompletionRegistry {
    providers: Vec<Arc<dyn InlineCompletionProvider>>,
    cache: CompletionCache,
    throttler: Throttler,
}

impl std::fmt::Debug for InlineCompletionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineCompletionRegistry")
            .field("providers", &self.providers.len())
            .finish_non_exhaustive()
    }
}

impl InlineCompletionRegistry {
    pub fn new(providers: Vec<Arc<dyn InlineCompletionProvider>>) -> Self {
        Self {
            providers,
            cache: CompletionCache::with_defaults(),
            throttler: Throttler::with_defaults(),
        }
    }

    pub fn cache(&self) -> &CompletionCache {
        &self.cache
    }

    pub fn throttler(&self) -> &Throttler {
        &self.throttler
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    pub async fn request(
        &self,
        req: InlineCompletionRequest,
    ) -> Result<InlineCompletionResponse, InlineCompletionError> {
        crate::observability::subsystem_metrics::incr_inline_completion_request();
        let key = CacheKey::from_context(&req.prefix, &req.suffix, req.language);
        let prefix_chars = req.prefix.chars().count();

        let prefix_hash = hash_cache_key(key);

        match self.throttler.try_acquire(prefix_chars, prefix_hash) {
            ThrottlerDecision::Allow => {}
            _ => {
                crate::observability::subsystem_metrics::incr_inline_completion_throttled();
                return Err(InlineCompletionError::Disabled {
                    reason: "throttler: rate-limited".into(),
                });
            }
        }

        if let Some(hit) = self.cache.get(key) {
            crate::observability::subsystem_metrics::incr_inline_completion_cache_hit();
            return Ok(InlineCompletionResponse {
                suggestions: vec![hit],
                latency_ms: 0,
                provider: "cache".into(),
                cached: true,
            });
        }
        crate::observability::subsystem_metrics::incr_inline_completion_cache_miss();

        let start = Instant::now();
        for p in &self.providers {
            if !p.supports(req.language) {
                continue;
            }
            match p.complete(req.clone()).await {
                Ok(mut resp) => {
                    if let Some(first) = resp.suggestions.first().cloned() {
                        self.cache.put(key, first);
                    }
                    resp.latency_ms = start.elapsed().as_millis() as u64;
                    global_stats().record_latency_ms(resp.latency_ms);
                    global_stats().record(AcceptanceEvent::Shown);
                    crate::observability::subsystem_metrics::observe_inline_completion_latency_ms(
                        resp.latency_ms,
                    );
                    return Ok(resp);
                }
                Err(InlineCompletionError::Empty { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(InlineCompletionError::Empty {
            provider: "registry".into(),
        })
    }
}

fn hash_cache_key(k: CacheKey) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    k.hash(&mut h);
    h.finish()
}

pub fn default_provider(config: &crate::config::Config) -> Option<RegistryHandle> {
    let provider_name = config.default_provider.clone()?;
    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let api_key = config.api_key.clone();
    let api_url = config.api_url.clone();
    let provider_timeout_secs = config.provider_timeout_secs;
    let extra_headers = config.extra_headers.clone();
    let api_path = config.api_path.clone();
    let provider_max_tokens = config.provider_max_tokens;
    let temperature = config.default_temperature;

    let runtime_options = crate::providers::ProviderRuntimeOptions {
        auth_profile_override: None,
        provider_api_url: api_url.clone(),
        sen_dir: config
            .config_path
            .parent()
            .map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: None,
        reasoning_effort: None,
        provider_timeout_secs: Some(provider_timeout_secs),
        extra_headers,
        api_path,
        provider_max_tokens,
    };
    let provider = crate::providers::create_provider_with_options(
        &provider_name,
        api_key.as_deref(),
        &runtime_options,
    )
    .ok()?;
    let provider: Arc<dyn crate::providers::Provider> = Arc::from(provider);

    let backend: super::providers::openai_style::ChatBackend = {
        let provider = provider.clone();
        let model = model.clone();
        Arc::new(move |prompt: String| {
            let provider = provider.clone();
            let model = model.clone();
            Box::pin(async move {
                provider
                    .chat_with_system(
                        Some(
                            "You are an expert code completion engine.  Reply with the \
                             insertion text only.  Never repeat the prefix or suffix.",
                        ),
                        &prompt,
                        &model,
                        temperature,
                    )
                    .await
            })
        })
    };

    let openai_provider = super::providers::OpenAiStyleProvider::new("default-chat", backend);
    let providers: Vec<Arc<dyn InlineCompletionProvider>> = vec![Arc::new(openai_provider)];
    Some(Arc::new(InlineCompletionRegistry::new(providers)))
}

#[derive(Debug)]
pub struct ScriptedProvider {
    pub fixed: String,
    pub support_all: bool,
}

#[async_trait]
impl InlineCompletionProvider for ScriptedProvider {
    async fn complete(
        &self,
        _req: InlineCompletionRequest,
    ) -> Result<InlineCompletionResponse, InlineCompletionError> {
        Ok(InlineCompletionResponse {
            suggestions: vec![super::traits::Suggestion {
                insert_text: self.fixed.clone(),
                rationale: None,
                confidence: Some(0.9),
            }],
            latency_ms: 0,
            provider: "scripted".into(),
            cached: false,
        })
    }
    fn name(&self) -> &'static str {
        "scripted"
    }
    fn supports(&self, _l: Language) -> bool {
        self.support_all
    }
}
