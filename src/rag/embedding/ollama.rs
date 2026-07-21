// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("rag.embedding.ollama", 300, 10)
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

    fn batch_embed_url(&self) -> String {
        if let Some(base) = self.endpoint.strip_suffix("/api/embeddings") {
            return format!("{base}/api/embed");
        }
        if self.endpoint.ends_with("/api/embed") {
            return self.endpoint.clone();
        }
        if self.endpoint.contains("/api/") {
            return format!("{}/embed", self.endpoint);
        }
        format!("{}/api/embed", self.endpoint)
    }

    async fn embed_batch(&self, texts: &[&str]) -> Option<Vec<Vec<f32>>> {
        let client = self.http_client();
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });
        let resp = client
            .post(self.batch_embed_url())
            .json(&body)
            .send()
            .await
            .ok()?;
        let status = resp.status();
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            tracing::debug!(
                %status,
                detail = %detail.chars().take(200).collect::<String>(),
                "ollama batch embed endpoint unavailable; falling back to per-text embeddings"
            );
            return None;
        }
        let parsed: OllamaBatchEmbedResponse = resp.json().await.ok()?;
        if parsed.embeddings.len() != texts.len() {
            return None;
        }
        Some(parsed.embeddings)
    }

    async fn embed_single(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let client = self.http_client();
        let policy = crate::util::retry::RetryPolicy::embedding();
        let model = self.model.clone();
        let url = self.embeddings_url();
        let text_owned = text.to_string();
        let client_ref = &client;
        let resp = crate::util::retry::retry(&policy, |attempt| {
            let model = model.clone();
            let url = url.clone();
            let text_owned = text_owned.clone();
            async move {
                let body = OllamaEmbedRequest {
                    model: &model,
                    prompt: &text_owned,
                };
                let resp = client_ref
                    .post(&url)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        anyhow::Error::new(e).context(format!(
                            "ollama embedding request failed on attempt {attempt}"
                        ))
                    })?;
                let status = resp.status();
                if status.is_server_error() || status.as_u16() == 429 {
                    let detail = resp.text().await.unwrap_or_default();
                    anyhow::bail!(
                        "ollama embedding transient {status} on attempt {attempt}: {}",
                        detail.chars().take(200).collect::<String>()
                    );
                }
                Ok::<reqwest::Response, anyhow::Error>(resp)
            }
        })
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            anyhow::bail!("ollama embedding api error {status}: {detail}");
        }
        let parsed: OllamaEmbedResponse = resp.json().await?;
        Ok(parsed.embedding)
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

    fn fingerprint(&self) -> String {
        format!("ollama:{}:{}", self.model, self.dims)
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(batch) = self.embed_batch(texts).await {
            return Ok(batch);
        }

        const MAX_CONCURRENT_SINGLE_EMBEDS: usize = 8;
        let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(MAX_CONCURRENT_SINGLE_EMBEDS) {
            let futures: Vec<_> = chunk.iter().map(|text| self.embed_single(text)).collect();
            for result in futures_util::future::join_all(futures).await {
                out.push(result?);
            }
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

#[derive(Debug, Deserialize)]
struct OllamaBatchEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}
