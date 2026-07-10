// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, StructuredResponse, TokenUsage, parse_first_json_object,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub const RESPONSES_API_MODEL_PREFIXES: &[&str] = &[
    "gpt-5", "gpt-4.1", "gpt-4o", "o1", "o3", "o4",
];

#[must_use]
pub fn model_uses_responses_api(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    RESPONSES_API_MODEL_PREFIXES
        .iter()
        .any(|p| m.starts_with(p))
}

pub struct OpenAiResponsesProvider {
    base_url: String,
    credential: Option<String>,
    max_output_tokens: Option<u32>,
    extra_headers: std::collections::HashMap<String, String>,
}

impl OpenAiResponsesProvider {
    #[must_use]
    pub fn new(credential: Option<&str>) -> Self {
        Self::with_base_url(None, credential)
    }

    #[must_use]
    pub fn with_base_url(base_url: Option<&str>, credential: Option<&str>) -> Self {
        Self {
            base_url: base_url
                .map(|u| u.trim_end_matches('/').to_string())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            credential: credential.map(ToString::to_string),
            max_output_tokens: None,
            extra_headers: std::collections::HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_max_output_tokens(mut self, max_output_tokens: Option<u32>) -> Self {
        self.max_output_tokens = max_output_tokens;
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

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts_and_headers(
                "provider.openai_responses",
                120,
                10,
                &self.extra_headers,
            )
    }

    fn adjust_temperature_for_model(model: &str, requested: f64) -> f64 {
        let m = model.trim().to_ascii_lowercase();
        let is_reasoning = m.starts_with("gpt-5")
            || m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4");
        if is_reasoning {
            1.0
        } else {
            requested
        }
    }
}

#[derive(Debug, Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    input: ResponsesInput<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'a str>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ResponsesInput<'a> {

    Text(&'a str),

    Messages(Vec<ResponsesMessage<'a>>),
}

#[derive(Debug, Serialize)]
struct ResponsesMessage<'a> {
    role: &'a str,
    content: ResponsesMessageContent<'a>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ResponsesMessageContent<'a> {
    Text(&'a str),
    Parts(Vec<ResponsesContentPart>),
}

#[derive(Debug, Serialize)]
struct ResponsesContentPart {
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_url: Option<String>,
}

fn user_message_content(content: &str) -> ResponsesMessageContent<'_> {
    let (cleaned_text, image_refs) = crate::multimodal::parse_image_markers(content);
    if image_refs.is_empty() {
        return ResponsesMessageContent::Text(content);
    }
    let mut parts = Vec::with_capacity(image_refs.len() + 1);
    if !cleaned_text.trim().is_empty() {
        parts.push(ResponsesContentPart {
            kind: "input_text".to_string(),
            text: Some(cleaned_text),
            image_url: None,
        });
    }
    for image_ref in image_refs {
        parts.push(ResponsesContentPart {
            kind: "input_image".to_string(),
            text: None,
            image_url: Some(image_ref),
        });
    }
    ResponsesMessageContent::Parts(parts)
}

#[derive(Debug, Serialize)]
struct ReasoningConfig {
    effort: String,
}

#[derive(Debug, Deserialize)]
struct ResponsesPayload {
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    output: Option<Vec<ResponsesOutputItem>>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    content: Option<Vec<ResponsesOutputContent>>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputContent {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    input_tokens_details: Option<ResponsesUsageDetails>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsageDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

impl ResponsesPayload {

    fn collect_text(&self) -> String {
        if let Some(t) = &self.output_text {
            if !t.is_empty() {
                return t.clone();
            }
        }

        let Some(items) = &self.output else {
            return String::new();
        };

        let mut buf = String::new();
        for item in items {

            if let Some(t) = &item.text {
                buf.push_str(t);
                continue;
            }

            let Some(parts) = &item.content else {
                continue;
            };
            for part in parts {
                if let Some(t) = &part.text {
                    if matches!(
                        part.kind.as_deref(),
                        Some("output_text") | Some("text") | None
                    ) {
                        buf.push_str(t);
                    }
                }
            }

            if matches!(item.kind.as_deref(), Some("message")) {
                buf.push('\n');
            }
        }
        buf.trim_end().to_string()
    }
}

#[async_trait]
impl Provider for OpenAiResponsesProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: false,
            vision: true,
            prompt_caching: true,
            responses_api: true,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml \
                 (provider = openai-responses)."
            )
        })?;

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);

        let request = ResponsesRequest {
            model,
            input: ResponsesInput::Text(message),
            instructions: system_prompt,
            temperature: adjusted_temperature,
            max_output_tokens: self.max_output_tokens,
            reasoning: None,
        };

        let response = self
            .http_client()
            .post(format!("{}/responses", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI Responses", response).await);
        }

        let payload: ResponsesPayload = response.json().await?;
        let text = payload.collect_text();
        if text.is_empty() {
            anyhow::bail!("OpenAI Responses returned an empty payload");
        }
        Ok(text)
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml \
                 (provider = openai-responses)."
            )
        })?;

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);

        let sanitized = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            self.max_output_tokens.unwrap_or(0) as usize,
            None,
        );

        let mut instructions: Option<&str> = None;
        let mut messages: Vec<ResponsesMessage<'_>> = Vec::new();
        for m in &sanitized {
            match m.role.as_str() {
                "system" => {
                    instructions = Some(m.content.as_str());
                }
                "user" => {
                    messages.push(ResponsesMessage {
                        role: "user",
                        content: user_message_content(m.content.as_str()),
                    });
                }
                "assistant" => {
                    messages.push(ResponsesMessage {
                        role: "assistant",
                        content: ResponsesMessageContent::Text(m.content.as_str()),
                    });
                }

                "tool" => {
                    messages.push(ResponsesMessage {
                        role: "assistant",
                        content: ResponsesMessageContent::Text(m.content.as_str()),
                    });
                }
                _ => {}
            }
        }

        let payload = ResponsesRequest {
            model,
            input: ResponsesInput::Messages(messages),
            instructions,
            temperature: adjusted_temperature,
            max_output_tokens: self.max_output_tokens,
            reasoning: None,
        };

        let response = self
            .http_client()
            .post(format!("{}/responses", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI Responses", response).await);
        }

        let parsed: ResponsesPayload = response.json().await?;
        let usage = parsed.usage.as_ref().map(|u| {
            let cached = u
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens);
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, None);
            TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: None,
            }
        });
        let text = parsed.collect_text();
        Ok(ProviderChatResponse::text_only(
            if text.is_empty() { None } else { Some(text) },
            usage,
        ))
    }

    async fn chat_structured(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<StructuredResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml \
                 (provider = openai-responses)."
            )
        })?;

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);

        let mut instructions: Option<&str> = None;
        let mut input_msgs: Vec<ResponsesMessage<'_>> = Vec::new();
        for m in messages {
            match m.role.as_str() {
                "system" => instructions = Some(m.content.as_str()),
                "user" => input_msgs.push(ResponsesMessage {
                    role: "user",
                    content: user_message_content(m.content.as_str()),
                }),
                "assistant" => input_msgs.push(ResponsesMessage {
                    role: "assistant",
                    content: ResponsesMessageContent::Text(m.content.as_str()),
                }),
                _ => {}
            }
        }

        let body = serde_json::json!({
            "model": model,
            "input": input_msgs,
            "instructions": instructions,
            "temperature": adjusted_temperature,
            "max_output_tokens": self.max_output_tokens,
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "structured_output",
                    "schema": schema,
                    "strict": true
                }
            }
        });

        let response = self
            .http_client()
            .post(format!("{}/responses", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI Responses", response).await);
        }

        let parsed: ResponsesPayload = response.json().await?;
        let usage = parsed.usage.as_ref().map(|u| {
            let cached = u
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens);
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, None);
            TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: None,
            }
        });
        let raw = parsed.collect_text();
        let value = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .or_else(|| parse_first_json_object(&raw))
            .ok_or_else(|| {
                anyhow::anyhow!("OpenAI Responses returned non-JSON payload: {raw}")
            })?;
        Ok(StructuredResponse {
            data: value,
            raw_text: raw,
            usage,
        })
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        if let Some(credential) = self.credential.as_ref() {
            self.http_client()
                .get(format!("{}/models", self.base_url))
                .header("Authorization", format!("Bearer {credential}"))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }
}
