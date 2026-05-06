// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Ollama embedding back-end.
//!
//! Speaks Ollama's [`/api/embeddings`](https://github.com/ollama/ollama/blob/main/docs/api.md#generate-embeddings)
//! REST surface.  Ollama handles one document per call, so the
//! implementation loops through the input batch and aggregates the
//! responses.  This matches Ollama's official behaviour and lets us
//! reuse the same trait shape as the OpenAI back-end without
//! pretending the local server supports batching.
//!
//! The embedding URL is derived from the configured endpoint:
//! - When the endpoint already ends with `/api/embeddings` we use
//!   it verbatim.
//! - When the endpoint already contains an `/api/` path we append
//!   `embeddings`.
//! - Otherwise we append `/api/embeddings` (the Ollama default).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{CodeEmbedderConfig, EmbeddingProvider};

pub struct OllamaCodeEmbedding {
    endpoint: String,
    model: String,
    dims: usize,
}

impl OllamaCodeEmbedding {
    pub fn from_config(cfg: &CodeEmbedderConfig) -> Self {
        let endpoint = cfg
            .endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            dims: cfg.dims,
        }
    }

    fn http_client(&self) -> reqwest::Client {
        crate::config::build_runtime_proxy_client("rag.embedding.ollama")
    }

    fn embeddings_url(&self) -> String {
        if self.endpoint.ends_with("/api/embeddings") {
            return self.endpoint.clone();
        }
        if self.endpoint.contains("/api/") {
            return format!("{}/embeddings", self.endpoint);
        }
        format!("{}/api/embeddings", self.endpoint)
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaCodeEmbedding {
    fn name(&self) -> &str {
        "ollama"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let client = self.http_client();
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let body = OllamaEmbedRequest {
                model: &self.model,
                prompt: text,
            };
            let resp = client
                .post(self.embeddings_url())
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let detail = resp.text().await.unwrap_or_default();
                anyhow::bail!("ollama embedding api error {status}: {detail}");
            }
            let parsed: OllamaEmbedResponse = resp.json().await?;
            out.push(parsed.embedding);
        }
        Ok(out)
    }
}

#[derive(Debug, Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    prompt: &'a str,
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}
