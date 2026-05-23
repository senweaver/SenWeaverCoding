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
        crate::services::get_services()
            .proxy_runtime()
            .build_client("rag.embedding.ollama")
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
        let policy = crate::util::retry::RetryPolicy::embedding();
        for text in texts {
            let model = self.model.clone();
            let url = self.embeddings_url();
            let text_owned = (*text).to_string();
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
