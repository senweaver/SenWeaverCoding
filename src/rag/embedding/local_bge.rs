// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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

    fn fingerprint(&self) -> String {
        format!("local_bge:{}:{}", self.model, self.dims)
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        anyhow::bail!(
            "local_bge embedding model '{model}' is not available  -  \
             enable the `rag-bge` feature or configure a remote embedder",
            model = self.model
        )
    }
}
