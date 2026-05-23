// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod local_bge;
pub mod ollama;
pub mod openai;

pub use crate::memory::embeddings::EmbeddingProvider;

#[derive(Debug, Clone)]
pub struct CodeEmbedderConfig {
    pub backend: CodeEmbedderBackend,
    pub model: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub dims: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeEmbedderBackend {
    OpenAi,
    Ollama,
    LocalBge,
}

impl CodeEmbedderConfig {
    pub fn openai(model: impl Into<String>, dims: usize, api_key: impl Into<String>) -> Self {
        Self {
            backend: CodeEmbedderBackend::OpenAi,
            model: model.into(),
            endpoint: Some("https://api.openai.com/v1".to_string()),
            api_key: Some(api_key.into()),
            dims,
        }
    }

    pub fn ollama(model: impl Into<String>, dims: usize) -> Self {
        Self {
            backend: CodeEmbedderBackend::Ollama,
            model: model.into(),
            endpoint: Some("http://localhost:11434".to_string()),
            api_key: None,
            dims,
        }
    }

    pub fn local_bge(model: impl Into<String>, dims: usize) -> Self {
        Self {
            backend: CodeEmbedderBackend::LocalBge,
            model: model.into(),
            endpoint: None,
            api_key: None,
            dims,
        }
    }

    pub fn with_endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = Some(url.into());
        self
    }
}

pub fn build_code_embedder(cfg: &CodeEmbedderConfig) -> Box<dyn EmbeddingProvider> {
    match cfg.backend {
        CodeEmbedderBackend::OpenAi => match openai::build(cfg) {
            Some(p) => p,
            None => Box::new(crate::memory::embeddings::NoopEmbedding),
        },
        CodeEmbedderBackend::Ollama => Box::new(ollama::OllamaCodeEmbedding::from_config(cfg)),
        CodeEmbedderBackend::LocalBge => Box::new(local_bge::LocalBgeEmbedding::from_config(cfg)),
    }
}
