// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared turn-setup helpers extracted from `Agent::turn` and
//! `Agent::turn_streamed` to reduce duplication without a full rewrite.
//!
//! The functions here are pure (no `Self`) and handle the "hot-reload
//! preamble" — the ~50 lines at the top of both turn variants that:
//! 1. Read the latest config from `shared_config`.
//! 2. Detect provider / api_key / api_url / model changes.
//! 3. Decide whether a provider reload is required.
//!
//! Centralizing this logic means a bug fix in the hot-reload path applies
//! to both `turn()` and `turn_streamed()` simultaneously.

use crate::security::secret_string::SecretString;

#[derive(Debug, Clone)]
pub struct ProviderSnapshot {
    pub provider: String,
    pub api_key: String,
    pub api_url: String,
    pub model: String,
}

impl ProviderSnapshot {

    pub fn from_config(
        config: &crate::config::schema::Config,
        default_model: impl FnOnce() -> String,
    ) -> Self {
        Self {
            provider: config
                .default_provider
                .clone()
                .unwrap_or_else(|| "openrouter".to_string()),
            api_key: config.api_key.clone().unwrap_or_default(),
            api_url: config.api_url.clone().unwrap_or_default(),
            model: config.default_model.clone().unwrap_or_else(default_model),
        }
    }

    pub fn diff(
        &self,
        cached_provider: &str,
        cached_api_key: &SecretString,
        cached_api_url: &str,
        cached_model: &str,
    ) -> ProviderDiff {
        ProviderDiff {
            provider_changed: self.provider != cached_provider,
            api_key_changed: !cached_api_key.constant_time_eq(&self.api_key),
            api_url_changed: self.api_url != cached_api_url,
            model_changed: self.model != cached_model,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderDiff {
    pub provider_changed: bool,
    pub api_key_changed: bool,
    pub api_url_changed: bool,
    pub model_changed: bool,
}

impl ProviderDiff {

    pub fn requires_provider_reload(self) -> bool {
        self.provider_changed || self.api_key_changed || self.api_url_changed
    }

    pub fn any(self) -> bool {
        self.provider_changed || self.api_key_changed || self.api_url_changed || self.model_changed
    }
}
