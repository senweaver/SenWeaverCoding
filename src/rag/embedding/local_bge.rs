// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! On-device BGE embedding placeholder.
//!
//! Shipping a real BGE checkpoint requires bundling a multi-hundred
//! MB ONNX or safetensors file plus an ONNX runtime, which is too
//! heavy for the default profile.  This module exposes the trait
//! contract today so call sites can plumb the configuration end to
//! end while leaving the actual inference to a future
//! `rag-bge`-feature-gated drop-in.
//!
//! When the feature is missing every embed call returns an error
//! that the caller must downgrade to a keyword-only search.  We
//! intentionally do not silently produce zero vectors — that would
//! pollute the IVF index with dummy entries that always score zero.

use async_trait::async_trait;

use super::{CodeEmbedderConfig, EmbeddingProvider};

pub struct LocalBgeEmbedding {
    model: String,
    dims: usize,
}

impl LocalBgeEmbedding {
    pub fn from_config(cfg: &CodeEmbedderConfig) -> Self {
        Self {
            model: cfg.model.clone(),
            dims: cfg.dims,
        }
    }
}

#[async_trait]
impl EmbeddingProvider for LocalBgeEmbedding {
    fn name(&self) -> &str {
        "local_bge"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        anyhow::bail!(
            "local_bge embedding model '{model}' is not available — \
             enable the `rag-bge` feature or configure a remote embedder",
            model = self.model
        )
    }
}
