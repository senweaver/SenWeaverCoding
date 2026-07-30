// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod audio;
pub mod image;
pub mod video;

use super::credentials::ResolvedProvider;

pub struct MediaJob {
    pub client: reqwest::Client,
    pub provider: ResolvedProvider,
    pub model: String,
    pub prompt: String,
    pub aspect: String,
    pub seconds: u32,
    pub voice: Option<String>,
    pub audio_kind: String,
    pub resolution: Option<String>,
    pub source_image: Option<std::path::PathBuf>,
    pub mask: Option<std::path::PathBuf>,
    pub fidelity: Option<String>,
}

impl MediaJob {
    pub fn require_key(&self) -> anyhow::Result<&str> {
        self.provider.api_key.as_deref().ok_or_else(|| {
            let env_hint = super::credentials::env_key_for(&self.provider.provider_id)
                .map(|var| format!(" or set the {var} environment variable"))
                .unwrap_or_default();
            anyhow::anyhow!(
                "No API key configured for provider '{id}' (base_url: {base}). Set \
                 `model_providers.{id}.api_key` in config / desktop Settings → Providers{env_hint}.",
                id = self.provider.provider_id,
                base = self.provider.base_url,
            )
        })
    }
}
