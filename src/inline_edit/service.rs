// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Process-wide factory + cache for [`InlineEditRunner`] used by the
//! TUI / GUI / CLI inline-edit entry points.
//!
//! Before M1.2 each surface had its own ad-hoc bridge (the TUI sent
//! a chat instruction to the agent, the GUI prototype called a
//! synchronous helper, the CLI did not exist).  This module
//! consolidates all of them on a single shared runner so:
//!
//! * the `Cmd+K` flow always goes through the apply / refine /
//!   verification pipeline,
//! * provider auth / timeout / rate-limit machinery is reused
//!   verbatim from [`crate::providers`],
//! * call sites can opt into the dedicated diff preview without
//!   round-tripping through the agent loop.
//!
//! The runner is constructed lazily from [`Config`] and cached in a
//! `OnceLock`.  `invalidate()` is exposed so the GUI/CLI can rebuild
//! the runner after the user changes provider settings.

use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use async_trait::async_trait;

use crate::apply_model::{FastApplyRefiner, HttpLlmRefiner, LlmRefiner};
use crate::config::Config;
use crate::inline_edit::runner::{InlineEditRunner, LlmClient, RunnerOptions};
use crate::providers::{self, Provider, ProviderRuntimeOptions};

struct ProviderLlmClient {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    name: &'static str,
}

#[async_trait]
impl LlmClient for ProviderLlmClient {
    async fn complete_diff(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, anyhow::Error> {
        self.provider
            .chat_with_system(
                Some(system_prompt),
                user_prompt,
                &self.model,
                self.temperature,
            )
            .await
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

fn cache() -> &'static RwLock<Option<Arc<InlineEditRunner>>> {
    static CACHE: OnceLock<RwLock<Option<Arc<InlineEditRunner>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn fast_refiner_cache() -> &'static RwLock<Option<Arc<FastApplyRefiner>>> {
    static CACHE: OnceLock<RwLock<Option<Arc<FastApplyRefiner>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn invalidate() {
    if let Ok(mut guard) = cache().write() {
        *guard = None;
    }
    if let Ok(mut guard) = fast_refiner_cache().write() {
        *guard = None;
    }
}

pub fn default_fast_refiner(config: &Config) -> Option<Arc<FastApplyRefiner>> {
    if let Ok(guard) = fast_refiner_cache().read()
        && let Some(existing) = guard.as_ref()
    {
        return Some(existing.clone());
    }
    let provider_name = config.default_provider.clone()?;
    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let runtime_options = ProviderRuntimeOptions {
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
        extra_headers: config.extra_headers.clone(),
        api_path: config.api_path.clone(),
        provider_max_tokens: config.provider_max_tokens,
    };
    let boxed = providers::create_provider_with_options(
        &provider_name,
        config.api_key.as_deref(),
        &runtime_options,
    )
    .ok()?;
    let provider: Arc<dyn Provider> = Arc::from(boxed);
    let full_refiner: Arc<dyn LlmRefiner> = Arc::new(
        HttpLlmRefiner::new(provider.clone(), model.clone())
            .with_temperature(config.default_temperature)
            .with_timeout(Duration::from_secs(config.provider_timeout_secs))
            .with_max_recursive_attempts(2),
    );
    let fast_runtime = &config.agent_runtime;
    let fast_refiner: Option<Arc<dyn LlmRefiner>> =
        match fast_runtime.fast_apply_model.as_deref() {
            Some(fast_model) if !fast_model.trim().is_empty() => {
                let timeout = Duration::from_secs(
                    fast_runtime.fast_apply_timeout_secs.max(1),
                );
                let r: Arc<dyn LlmRefiner> = Arc::new(
                    HttpLlmRefiner::new(provider.clone(), fast_model.to_string())
                        .with_temperature(fast_runtime.fast_apply_temperature)
                        .with_timeout(timeout)
                        .with_max_recursive_attempts(1),
                );
                Some(r)
            }
            _ => None,
        };
    let tiered = Arc::new(FastApplyRefiner::new(fast_refiner, full_refiner));
    if let Ok(mut guard) = fast_refiner_cache().write() {
        *guard = Some(tiered.clone());
    }
    Some(tiered)
}

pub fn default_runner(config: &Config) -> Option<Arc<InlineEditRunner>> {
    if let Ok(guard) = cache().read()
        && let Some(existing) = guard.as_ref()
    {
        return Some(existing.clone());
    }

    let provider_name = config.default_provider.clone()?;
    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    let runtime_options = ProviderRuntimeOptions {
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
        extra_headers: config.extra_headers.clone(),
        api_path: config.api_path.clone(),
        provider_max_tokens: config.provider_max_tokens,
    };

    let boxed = providers::create_provider_with_options(
        &provider_name,
        config.api_key.as_deref(),
        &runtime_options,
    )
    .ok()?;
    let provider: Arc<dyn Provider> = Arc::from(boxed);

    let llm: Arc<dyn LlmClient> = Arc::new(ProviderLlmClient {
        provider: provider.clone(),
        model: model.clone(),
        temperature: config.default_temperature,
        name: "inline_edit_runner",
    });

    let tiered = default_fast_refiner(config)?;

    let runner = Arc::new(
        InlineEditRunner::new(llm, RunnerOptions::default()).with_fast_refiner(tiered),
    );
    if let Ok(mut guard) = cache().write() {
        *guard = Some(runner.clone());
    }
    Some(runner)
}
