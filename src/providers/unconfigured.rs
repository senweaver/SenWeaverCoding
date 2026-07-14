// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;

use super::traits::{ChatMessage, ChatRequest, ChatResponse, Provider, ProviderCapabilities};

const ERR: &str = "No LLM provider is configured. Open Settings → Providers, add an API key \
(or OAuth), then retry. The gateway is running in setup mode so the desktop shell can stay open.";

pub struct UnconfiguredProvider {
    reason: String,
}

impl UnconfiguredProvider {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn fail<T>(&self) -> anyhow::Result<T> {
        anyhow::bail!("{ERR} ({})", self.reason)
    }
}

#[async_trait]
impl Provider for UnconfiguredProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        self.fail()
    }

    async fn chat_with_history(
        &self,
        _messages: &[ChatMessage],
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        self.fail()
    }

    async fn chat(
        &self,
        _request: ChatRequest<'_>,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        self.fail()
    }

    async fn chat_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: &[serde_json::Value],
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        self.fail()
    }
}
