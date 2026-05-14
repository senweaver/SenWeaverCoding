// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! registry helper that fans a [`NepRequest`] across multiple
//! [`NepProvider`]s and returns the first non-empty response.
//!
//! Mirrors the shape of
//! [`crate::inline_completion::registry::InlineCompletionRegistry`]
//! so the surfaces (TUI / GUI / CLI) can store a single handle and
//! call `predict` regardless of how many providers are configured.

use std::sync::Arc;
use std::time::Instant;

use super::{NepError, NepHandle, NepRequest, NepResponse};

pub struct NepRegistry {
    providers: Vec<NepHandle>,

    fallback: Option<NepHandle>,
}

impl NepRegistry {
    pub fn new(providers: Vec<NepHandle>) -> Self {
        Self {
            providers,
            fallback: None,
        }
    }

    #[must_use]
    pub fn with_fallback(mut self, fallback: NepHandle) -> Self {
        self.fallback = Some(fallback);
        self
    }

    pub async fn predict(&self, req: NepRequest) -> Result<NepResponse, NepError> {
        let start = Instant::now();
        let mut last_error: Option<NepError> = None;
        for provider in &self.providers {
            match provider.predict(req.clone()).await {
                Ok(response) if !response.suggestions.is_empty() => {
                    return Ok(response);
                }
                Ok(_) => continue,
                Err(err) => {
                    tracing::debug!(
                        target: "nep.registry",
                        provider = provider.name(),
                        error = %err,
                        "nep provider error; trying next",
                    );
                    last_error = Some(err);
                    continue;
                }
            }
        }
        if let Some(fallback) = self.fallback.as_ref() {
            match fallback.predict(req).await {
                Ok(mut response) if !response.suggestions.is_empty() => {
                    response.latency_ms = start.elapsed().as_millis() as u64;
                    return Ok(response);
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::debug!(
                        target: "nep.registry",
                        provider = fallback.name(),
                        error = %err,
                        "fallback nep provider failed",
                    );
                    last_error = Some(err);
                }
            }
        }
        if let Some(err) = last_error {
            return Err(err);
        }
        Ok(NepResponse {
            suggestions: Vec::new(),
            latency_ms: start.elapsed().as_millis() as u64,
            provider: "nep_registry_empty".into(),
        })
    }
}

pub fn default_registry(config: &crate::config::Config) -> Arc<NepRegistry> {
    let heuristic: NepHandle = Arc::new(super::HeuristicNep::new());
    let mut providers: Vec<NepHandle> = vec![heuristic.clone()];
    if let Some(provider_name_raw) = config.default_provider.clone() {
        let provider_name =
            crate::providers::resolve_runtime_provider_name(&provider_name_raw, config);
        let model = match config
            .agent_runtime
            .fast_apply_model
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| config.default_model.clone().filter(|s| !s.trim().is_empty()))
        {
            Some(m) => m,
            None => match crate::providers::resolve_default_model(config) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(
                        target = "config",
                        "no_model_configured: skipping nep default_registry: {e}"
                    );
                    return Arc::new(NepRegistry::new(providers).with_fallback(heuristic));
                }
            },
        };
        let runtime_options = crate::providers::ProviderRuntimeOptions {
            auth_profile_override: None,
            provider_api_url: config.api_url.clone(),
            sen_dir: config
                .config_path
                .parent()
                .map(std::path::PathBuf::from),
            secrets_encrypt: config.secrets.encrypt,
            reasoning_enabled: None,
            reasoning_effort: None,
            provider_timeout_secs: Some(config.provider_timeout_secs),
            extra_headers: crate::providers::merged_extra_headers_for_config(config),
            api_path: config.api_path.clone(),
            provider_max_tokens: config.provider_max_tokens,
            model_context_windows: config.model_context_windows.clone(),
        };
        if let Ok(boxed) = crate::providers::create_provider_with_options(
            &provider_name,
            config.api_key.as_deref(),
            &runtime_options,
        ) {
            let provider: Arc<dyn crate::providers::Provider> = Arc::from(boxed);
            let llm = super::LlmNep::new(provider, model);
            providers.push(Arc::new(llm));
        }
    }
    Arc::new(NepRegistry::new(providers).with_fallback(heuristic))
}
