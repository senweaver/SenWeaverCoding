// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use futures_util::stream;
use std::sync::Arc;

use crate::inline_completion::traits::{
    CompletionStream, InlineCompletionError, InlineCompletionProvider, InlineCompletionRequest,
    InlineCompletionResponse, Language, Suggestion,
};

pub type ChatBackend = Arc<
    dyn Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, anyhow::Error>> + Send>,
        > + Send
        + Sync,
>;

pub type ChatStreamBackend = Arc<
    dyn Fn(
            String,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<CompletionStream, anyhow::Error>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

#[derive(Clone)]
pub struct OpenAiStyleProvider {
    backend: ChatBackend,
    stream_backend: Option<ChatStreamBackend>,
    name: &'static str,
}

impl std::fmt::Debug for OpenAiStyleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiStyleProvider")
            .field("name", &self.name)
            .field("streaming", &self.stream_backend.is_some())
            .finish_non_exhaustive()
    }
}

impl OpenAiStyleProvider {
    pub fn new(name: &'static str, backend: ChatBackend) -> Self {
        Self {
            backend,
            stream_backend: None,
            name,
        }
    }

    #[must_use]
    pub fn with_stream_backend(mut self, backend: ChatStreamBackend) -> Self {
        self.stream_backend = Some(backend);
        self
    }

    pub fn build_prompt(req: &InlineCompletionRequest) -> String {
        format!(
            "You are an expert {lang:?} code completion engine.\n\
             Continue the code at the cursor.  Do not repeat the prefix\n\
             or suffix.  Respond with the insertion text only.\n\n\
             --- prefix ---\n{p}\n--- cursor ---\n--- suffix ---\n{s}\n\n\
             Insertion:",
            lang = req.language,
            p = req.prefix,
            s = req.suffix
        )
    }
}

#[async_trait]
impl InlineCompletionProvider for OpenAiStyleProvider {
    async fn complete(
        &self,
        req: InlineCompletionRequest,
    ) -> Result<InlineCompletionResponse, InlineCompletionError> {
        let prompt = Self::build_prompt(&req);
        let raw = (self.backend)(prompt)
            .await
            .map_err(|e| InlineCompletionError::Provider {
                provider: self.name.to_string(),
                source: e,
            })?;
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            return Err(InlineCompletionError::Empty {
                provider: self.name.to_string(),
            });
        }
        Ok(InlineCompletionResponse {
            suggestions: vec![Suggestion {
                insert_text: trimmed,
                rationale: Some("openai_style".into()),
                confidence: None,
            }],
            latency_ms: 0,
            provider: self.name.to_string(),
            cached: false,
        })
    }

    async fn stream_complete(
        &self,
        req: InlineCompletionRequest,
    ) -> Result<CompletionStream, InlineCompletionError> {
        let Some(backend) = self.stream_backend.clone() else {
            let resp = self.complete(req).await?;
            let full = resp
                .suggestions
                .into_iter()
                .next()
                .map(|s| s.insert_text)
                .unwrap_or_default();
            return Ok(Box::pin(stream::once(async move { Ok(full) })));
        };

        let prompt = Self::build_prompt(&req);
        let inner =
            backend(prompt)
                .await
                .map_err(|e| InlineCompletionError::Provider {
                    provider: self.name.to_string(),
                    source: e,
                })?;
        Ok(inner)
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn supports(&self, _l: Language) -> bool {
        true
    }
}
