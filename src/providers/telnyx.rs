// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{ChatMessage, Provider};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub struct TelnyxProvider {

    api_key: Option<String>,

    base_url: String,

    extra_headers: std::collections::HashMap<String, String>,

    client: Client,
}

impl TelnyxProvider {

    const BASE_URL: &'static str = "https://api.telnyx.com/v2/ai";

    const DEFAULT_TIMEOUT_SECS: u64 = 120;

    pub fn new(api_key: Option<&str>) -> Self {
        Self::with_base_url(api_key, Self::BASE_URL)
    }

    pub fn with_base_url(api_key: Option<&str>, base_url: &str) -> Self {
        let resolved_key = resolve_telnyx_api_key(api_key);
        let base_url = base_url.trim().trim_end_matches('/');
        let base_url = if base_url.is_empty() {
            Self::BASE_URL.to_string()
        } else {
            base_url.to_string()
        };
        Self {
            api_key: resolved_key,
            base_url,
            extra_headers: std::collections::HashMap::new(),
            client: Self::build_client(Self::DEFAULT_TIMEOUT_SECS),
        }
    }

    fn build_client(timeout_secs: u64) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("provider.telnyx", timeout_secs.max(1), 10)
    }

    #[must_use]
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.client = Self::build_client(timeout_secs);
        self
    }

    #[must_use]
    pub fn with_extra_headers(
        mut self,
        headers: std::collections::HashMap<String, String>,
    ) -> Self {
        self.extra_headers = headers;
        self
    }

    fn apply_extra_headers(&self, mut request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        for (name, value) in &self.extra_headers {
            request = request.header(name, value);
        }
        request
    }

    pub async fn list_models(&self) -> anyhow::Result<Vec<String>> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Telnyx API key not set. Set TELNYX_API_KEY environment variable.")
        })?;

        let request = self
            .client
            .get(format!("{}/models", self.base_url))
            .header("Authorization", format!("Bearer {}", api_key));
        let response = self.apply_extra_headers(request).send().await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to list Telnyx models: {}", error);
        }

        let models_response: ModelsResponse = response.json().await?;
        Ok(models_response.data.into_iter().map(|m| m.id).collect())
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
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
    #[serde(default)]
    content: Option<String>,
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

        let http_request = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request);
        let response = self.apply_extra_headers(http_request).send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("Telnyx", response).await);
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.unwrap_or_default())
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

        let sanitized = super::traits::flatten_messages_for_text_only_wire(messages);
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

        let http_request = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request);
        let response = self.apply_extra_headers(http_request).send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("Telnyx", response).await);
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.unwrap_or_default())
            .ok_or_else(|| anyhow::anyhow!("No response from Telnyx"))
    }

    async fn warmup(&self) -> anyhow::Result<()> {

        let _ = self
            .client
            .get(format!("{}/models", self.base_url))
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
