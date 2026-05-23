// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::{CodeEmbedderConfig, EmbeddingProvider};

pub fn build(cfg: &CodeEmbedderConfig) -> Option<Box<dyn EmbeddingProvider>> {
    let api_key = cfg.api_key.as_deref()?;
    let base_url = cfg
        .endpoint
        .as_deref()
        .unwrap_or("https://api.openai.com/v1");
    let provider = crate::memory::embeddings::OpenAiEmbedding::new(
        base_url,
        api_key,
        &cfg.model,
        cfg.dims,
    );
    Some(Box::new(provider))
}
