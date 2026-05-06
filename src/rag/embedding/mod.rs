// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Code-RAG embedding providers.
//!
//! The `memory::embeddings` module exposes a generic
//! [`crate::memory::embeddings::EmbeddingProvider`] trait used by the
//! long-term-memory store.  Code RAG has different defaults
//! (different recommended models, different default endpoints, code
//! chunking) so we surface a thin wrapper specifically for the
//! [`crate::rag::vector_code_index`] pipeline.
//!
//! Three back-ends ship out of the box:
//!
//! | provider     | endpoint                               | typical model            |
//! |--------------|----------------------------------------|--------------------------|
//! | `openai`     | `https://api.openai.com/v1`            | `text-embedding-3-small` |
//! | `ollama`     | `http://localhost:11434`               | `nomic-embed-text`       |
//! | `local_bge`  | in-process (placeholder until feature) | `bge-small-en-v1.5`      |
//!
//! The factory [`build_code_embedder`] returns a boxed provider that
//! can be plugged into [`crate::rag::vector_code_index::VectorCodeIndex`]
//! and surfaced through
//! [`crate::agent::loop_services::rag_source`].
//!
//! Notes
//! - The OpenAI back-end shares its HTTP client with
//!   [`crate::memory::embeddings::OpenAiEmbedding`] so we don't
//!   duplicate proxy / retry plumbing.
//! - The Ollama back-end speaks Ollama's `/api/embeddings` REST shape
//!   (one document per call) — fine for code-search workloads where
//!   indexing is amortised across many calls.
//! - `local_bge` is intentionally a stub for now: shipping a real
//!   on-device BGE checkpoint requires a feature gate (`rag-bge`) so
//!   the default profile keeps the binary small.  Calls degrade to
//!   `NotImplemented` so call sites can fall back to keyword search.

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
