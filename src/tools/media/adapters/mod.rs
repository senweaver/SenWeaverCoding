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
            anyhow::anyhow!(
                "No API key configured for provider '{}'. Add it under model providers settings.",
                self.provider.provider_id
            )
        })
    }
}
