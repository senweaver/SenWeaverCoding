// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use async_trait::async_trait;

use crate::providers::Provider;

use super::traits::{AgentHandle, FlowError};

pub struct ProviderAgentHandle {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
}

impl ProviderAgentHandle {
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>, temperature: f64) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature,
        }
    }
}

#[async_trait]
impl AgentHandle for ProviderAgentHandle {
    async fn complete(&self, prompt: &str) -> Result<String, FlowError> {
        self.provider
            .simple_chat(prompt, &self.model, self.temperature)
            .await
            .map_err(|e| FlowError::AgentHandle(e.to_string()))
    }
}
