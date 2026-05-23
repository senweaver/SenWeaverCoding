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

pub type FimBackend = Arc<
    dyn Fn(
            FimPrompt,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, anyhow::Error>> + Send>,
        > + Send
        + Sync,
>;

pub type FimStreamBackend = Arc<
    dyn Fn(
            FimPrompt,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<CompletionStream, anyhow::Error>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

#[derive(Debug, Clone)]
pub struct FimPrompt {
    pub prefix: String,
    pub suffix: String,
    pub max_tokens: u32,
    pub stop: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FimStyle {

    PrefixSuffixMiddle,

    StarCoder,
}

#[derive(Clone)]
pub struct FimProvider {
    backend: FimBackend,
    stream_backend: Option<FimStreamBackend>,
    style: FimStyle,
    name: &'static str,
}

impl std::fmt::Debug for FimProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FimProvider")
            .field("style", &self.style)
            .field("name", &self.name)
            .field("streaming", &self.stream_backend.is_some())
            .finish_non_exhaustive()
    }
}

impl FimProvider {
    pub fn new(name: &'static str, style: FimStyle, backend: FimBackend) -> Self {
        Self {
            backend,
            stream_backend: None,
            style,
            name,
        }
    }

    #[must_use]
    pub fn with_stream_backend(mut self, backend: FimStreamBackend) -> Self {
        self.stream_backend = Some(backend);
        self
    }

    pub fn build_prompt(style: FimStyle, req: &InlineCompletionRequest) -> String {
        match style {
            FimStyle::PrefixSuffixMiddle => format!(
                "<fim_prefix>{}<fim_suffix>{}<fim_middle>",
                req.prefix, req.suffix
            ),
            FimStyle::StarCoder => format!("<PRE>{}<SUF>{}<MID>", req.prefix, req.suffix),
        }
    }

    pub fn trim_at_stop(raw: &str, stop_sequences: &[String]) -> String {
        let mut best: Option<usize> = None;
        for s in stop_sequences {
            if s.is_empty() {
                continue;
            }
            if let Some(i) = raw.find(s.as_str())
                && best.is_none_or(|b| i < b)
            {
                best = Some(i);
            }
        }
        match best {
            Some(i) => raw[..i].to_string(),
            None => raw.to_string(),
        }
    }
}

#[async_trait]
impl InlineCompletionProvider for FimProvider {
    async fn complete(
        &self,
        req: InlineCompletionRequest,
    ) -> Result<InlineCompletionResponse, InlineCompletionError> {
        let prompt = FimPrompt {
            prefix: req.prefix.clone(),
            suffix: req.suffix.clone(),
            max_tokens: req.max_tokens,
            stop: req.stop_sequences.clone(),
        };
        let raw = (self.backend)(prompt)
            .await
            .map_err(|e| InlineCompletionError::Provider {
                provider: self.name.to_string(),
                source: e,
            })?;
        let trimmed = Self::trim_at_stop(&raw, &req.stop_sequences);
        if trimmed.trim().is_empty() {
            return Err(InlineCompletionError::Empty {
                provider: self.name.to_string(),
            });
        }
        Ok(InlineCompletionResponse {
            suggestions: vec![Suggestion {
                insert_text: trimmed,
                rationale: Some(format!("fim:{:?}", self.style)),
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

        let prompt = FimPrompt {
            prefix: req.prefix.clone(),
            suffix: req.suffix.clone(),
            max_tokens: req.max_tokens,
            stop: req.stop_sequences.clone(),
        };
        let inner =
            backend(prompt)
                .await
                .map_err(|e| InlineCompletionError::Provider {
                    provider: self.name.to_string(),
                    source: e,
                })?;
        let stops = req.stop_sequences.clone();
        let mut acc = String::new();
        let mut emitted = 0usize;
        let mut halted = false;
        let mapped = futures_util::StreamExt::filter_map(inner, move |item| {
            let result = match item {
                Ok(chunk) => {
                    if halted {
                        None
                    } else {
                        acc.push_str(&chunk);
                        if let Some((cut, hit)) = first_stop_match(&acc, &stops) {
                            halted = true;
                            let trimmed = acc[..cut].to_string();
                            let new_part = if trimmed.len() > emitted {
                                Some(trimmed[emitted..].to_string())
                            } else {
                                None
                            };
                            emitted = trimmed.len();
                            let _ = hit;
                            new_part.map(Ok)
                        } else {
                            Some(Ok(chunk))
                        }
                    }
                }
                Err(e) => Some(Err(e)),
            };
            async move { result }
        });
        Ok(Box::pin(mapped))
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn supports(&self, lang: Language) -> bool {
        lang.is_code()
    }
}

fn first_stop_match(text: &str, stops: &[String]) -> Option<(usize, String)> {
    let mut best: Option<(usize, String)> = None;
    for s in stops {
        if s.is_empty() {
            continue;
        }
        if let Some(idx) = text.find(s.as_str()) {
            match &best {
                Some((b, _)) if idx >= *b => {}
                _ => best = Some((idx, s.clone())),
            }
        }
    }
    best
}
