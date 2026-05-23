// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::pin::Pin;

use super::context_builder::InlineContext;

pub type CompletionStream =
    Pin<Box<dyn Stream<Item = Result<String, InlineCompletionError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Ruby,
    Php,
    Swift,
    Kotlin,
    Scala,
    Html,
    Css,
    Json,
    Toml,
    Yaml,
    Markdown,
    Sql,
    Shell,

    Other,
}

impl Language {

    pub fn from_extension(ext: &str) -> Self {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "rs" => Self::Rust,
            "ts" | "tsx" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "py" | "pyi" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Self::Cpp,
            "cs" => Self::CSharp,
            "rb" => Self::Ruby,
            "php" => Self::Php,
            "swift" => Self::Swift,
            "kt" | "kts" => Self::Kotlin,
            "scala" | "sc" => Self::Scala,
            "html" | "htm" => Self::Html,
            "css" | "scss" | "less" => Self::Css,
            "json" | "jsonc" => Self::Json,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "md" | "markdown" => Self::Markdown,
            "sql" => Self::Sql,
            "sh" | "bash" | "zsh" | "fish" | "ps1" => Self::Shell,
            _ => Self::Other,
        }
    }

    pub fn is_code(&self) -> bool {
        !matches!(self, Self::Markdown | Self::Other)
    }
}

#[derive(Debug, Clone)]
pub struct InlineCompletionRequest {

    pub prefix: String,

    pub suffix: String,
    pub language: Language,
    pub file_path: PathBuf,
    pub workspace_root: PathBuf,
    pub context: InlineContext,

    pub max_tokens: u32,

    pub stop_sequences: Vec<String>,

    pub request_id: uuid::Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {

    pub insert_text: String,

    pub rationale: Option<String>,

    pub confidence: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct InlineCompletionResponse {

    pub suggestions: Vec<Suggestion>,

    pub latency_ms: u64,

    pub provider: String,

    pub cached: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum InlineCompletionError {
    #[error("provider {provider} returned no suggestion")]
    Empty { provider: String },
    #[error("provider {provider} timed out after {timeout_ms}ms")]
    Timeout { provider: String, timeout_ms: u64 },
    #[error("provider {provider} failed: {source}")]
    Provider {
        provider: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("completion disabled: {reason}")]
    Disabled { reason: String },
}

#[async_trait]
pub trait InlineCompletionProvider: Send + Sync {

    async fn complete(
        &self,
        req: InlineCompletionRequest,
    ) -> Result<InlineCompletionResponse, InlineCompletionError>;

    async fn stream_complete(
        &self,
        req: InlineCompletionRequest,
    ) -> Result<CompletionStream, InlineCompletionError> {
        let resp = self.complete(req).await?;
        let full = resp
            .suggestions
            .into_iter()
            .next()
            .map(|s| s.insert_text)
            .unwrap_or_default();
        let stream = futures_util::stream::once(async move { Ok(full) });
        Ok(Box::pin(stream))
    }

    fn name(&self) -> &'static str;

    fn supports(&self, _lang: Language) -> bool {
        true
    }
}
