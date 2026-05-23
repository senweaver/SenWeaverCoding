// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{ChatMessage, Provider};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub struct TelnyxProvider {

    api_key: Option<String>,

    client: Client,
}

impl TelnyxProvider {

    const BASE_URL: &'static str = "https://api.telnyx.com/v2/ai";

    pub fn new(api_key: Option<&str>) -> Self {
        let resolved_key = resolve_telnyx_api_key(api_key);
        Self {
            api_key: resolved_key,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub fn with_base_url(api_key: Option<&str>, _base_url: &str) -> Self {

        Self::new(api_key)
    }

    pub async fn list_models(&self) -> anyhow::Result<Vec<String>> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Telnyx API key not set. Set TELNYX_API_KEY environment variable.")
        })?;

        let response = self
            .client
            .get(format!("{}/models", Self::BASE_URL))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to list Telnyx models: {}", error);
        }

        let models_response: ModelsResponse = response.json().await?;
        Ok(models_response.data.into_iter().map(|m| m.id).collect())
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", Self::BASE_URL)
    }
}

fn resolve_telnyx_api_key(api_key: Option<&str>) -> Option<String> {
    if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        return Some(key.to_string());
    }

    if let Ok(key) = std::env::var("TELNYX_API_KEY") {
        let key = key.trim();
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }

    for env_var in ["SEN_API_KEY", "API_KEY"] {
        if let Ok(key) = std::env::var(env_var) {
            let key = key.trim();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
    }

    None
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Debug, serde::Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
}

#[derive(Debug, serde::Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

#[async_trait]
impl Provider for TelnyxProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Telnyx API key not set. Set TELNYX_API_KEY environment variable or run `sen onboard`."
            )
        })?;

        let mut messages = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system".to_string(),
                content: sys.to_string(),
            });
        }

        messages.push(Message {
            role: "user".to_string(),
            content: message.to_string(),
        });

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
        };

        let response = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await?;
            let sanitized = super::sanitize_api_error(&error);
            anyhow::bail!("Telnyx API error ({}): {}", status, sanitized);
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow::anyhow!("No response from Telnyx"))
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Telnyx API key not set. Set TELNYX_API_KEY environment variable or run `sen onboard`."
            )
        })?;

        let sanitized = super::traits::sanitize_messages_for_legacy(messages);
        let budgeted = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            sanitized,
            model,
            0,
            None,
        );

        let api_messages: Vec<Message> = budgeted
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let request = ChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature,
        };

        let response = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await?;
            let sanitized = super::sanitize_api_error(&error);
            anyhow::bail!("Telnyx API error ({}): {}", status, sanitized);
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow::anyhow!("No response from Telnyx"))
    }

    async fn warmup(&self) -> anyhow::Result<()> {

        let _ = self
            .client
            .get(format!("{}/models", Self::BASE_URL))
            .send()
            .await;
        Ok(())
    }
}

pub mod models {

    pub const GPT_4O: &str = "openai/gpt-4o";

    pub const GPT_4O_MINI: &str = "openai/gpt-4o-mini";

    pub const GPT_4_TURBO: &str = "openai/gpt-4-turbo";

    pub const CLAUDE_3_5_SONNET: &str = "anthropic/claude-3.5-sonnet";

    pub const LLAMA_3_1_70B: &str = "meta-llama/llama-3.1-70b-instruct";

    pub const LLAMA_3_1_8B: &str = "meta-llama/llama-3.1-8b-instruct";

    pub const MISTRAL_LARGE: &str = "mistralai/mistral-large";

    pub const MISTRAL_SMALL: &str = "mistralai/mistral-small";
}
