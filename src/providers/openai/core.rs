// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, StreamChunk, StreamError, StreamEvent, StreamOptions, StreamResult,
    StructuredResponse, TokenUsage, ToolCall as ProviderToolCall, parse_first_json_object,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OpenAiProvider {
    base_url: String,
    credential: Option<String>,
    max_tokens: Option<u32>,
    timeout_secs: u64,
    extra_headers: std::collections::HashMap<String, String>,
}

const DEFAULT_OPENAI_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptionsField>,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct StreamOptionsField {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
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

    #[serde(default)]
    reasoning_content: Option<String>,
}

impl ResponseMessage {
    fn effective_content(&self) -> String {
        match &self.content {
            Some(c) if !c.is_empty() => c.clone(),
            _ => self.reasoning_content.clone().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Serialize)]
struct NativeChatRequest {
    model: String,
    messages: Vec<NativeMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<NativeToolSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptionsField>,
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "crate::providers::sanitize::skip_serializing_tool_calls")]
    tool_calls: Option<Vec<NativeToolCall>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeToolSpec {
    #[serde(rename = "type")]
    kind: String,
    function: NativeToolFunctionSpec,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeToolFunctionSpec {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

fn parse_native_tool_spec(value: serde_json::Value) -> anyhow::Result<NativeToolSpec> {
    let spec: NativeToolSpec = serde_json::from_value(value)
        .map_err(|e| anyhow::anyhow!("Invalid OpenAI tool specification: {e}"))?;

    if spec.kind != "function" {
        anyhow::bail!(
            "Invalid OpenAI tool specification: unsupported tool type '{}', expected 'function'",
            spec.kind
        );
    }

    Ok(spec)
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    function: NativeFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct NativeChatResponse {
    choices: Vec<NativeChoice>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NativeChoice {
    message: NativeResponseMessage,
}

#[derive(Debug, Deserialize)]
struct NativeResponseMessage {
    #[serde(default)]
    content: Option<String>,

    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<NativeToolCall>>,
}

impl NativeResponseMessage {
    fn effective_content(&self) -> Option<String> {
        match &self.content {
            Some(c) if !c.is_empty() => Some(c.clone()),
            _ => self.reasoning_content.clone(),
        }
    }
}

impl OpenAiProvider {
    pub fn new(credential: Option<&str>) -> Self {
        Self::with_base_url(None, credential)
    }

    pub fn with_base_url(base_url: Option<&str>, credential: Option<&str>) -> Self {
        Self {
            base_url: base_url
                .map(|u| u.trim_end_matches('/').to_string())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            credential: credential.map(ToString::to_string),
            max_tokens: None,
            timeout_secs: DEFAULT_OPENAI_TIMEOUT_SECS,
            extra_headers: std::collections::HashMap::new(),
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    pub fn with_extra_headers(
        mut self,
        headers: std::collections::HashMap<String, String>,
    ) -> Self {
        self.extra_headers = headers;
        self
    }

    // OpenAI reasoning-family models (o1/o3/o4/gpt-5*) reject `max_tokens` and
    // require `max_completion_tokens` instead.
    fn uses_max_completion_tokens(model: &str) -> bool {
        let id = model.rsplit('/').next().unwrap_or(model);
        id.starts_with("o1")
            || id.starts_with("o3")
            || id.starts_with("o4")
            || id.starts_with("gpt-5")
    }

    fn max_tokens_field(max: Option<u32>, model: &str) -> Option<u32> {
        if Self::uses_max_completion_tokens(model) {
            None
        } else {
            max
        }
    }

    fn max_completion_tokens_field(max: Option<u32>, model: &str) -> Option<u32> {
        if Self::uses_max_completion_tokens(model) {
            max
        } else {
            None
        }
    }

    fn adjust_temperature_for_model(model: &str, requested_temperature: f64) -> f64 {

        let requires_1_0 = matches!(
            model,
            "gpt-5"
                | "gpt-5-2025-08-07"
                | "gpt-5-mini"
                | "gpt-5-mini-2025-08-07"
                | "gpt-5-nano"
                | "gpt-5-nano-2025-08-07"
                | "gpt-5.1-chat-latest"
                | "gpt-5.2-chat-latest"
                | "gpt-5.3-chat-latest"
                | "o1"
                | "o1-2024-12-17"
                | "o3"
                | "o3-2025-04-16"
                | "o3-mini"
                | "o3-mini-2025-01-31"
                | "o4-mini"
                | "o4-mini-2025-04-16"
        );

        if requires_1_0 {
            1.0
        } else {
            requested_temperature
        }
    }

    fn convert_tools(tools: Option<&[ToolSpec]>) -> Option<Vec<NativeToolSpec>> {
        tools.map(|items| {
            crate::tools::dedupe_tool_specs(items)
                .iter()
                .map(|tool| NativeToolSpec {
                    kind: "function".to_string(),
                    function: NativeToolFunctionSpec {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.parameters.clone(),
                    },
                })
                .collect()
        })
    }

    fn convert_messages(messages: &[ChatMessage]) -> Vec<NativeMessage> {
        messages
            .iter()
            .map(|m| {
                if m.role == "assistant" {
                    let trimmed = m.content.trim_start();
                    if trimmed.starts_with('[') {
                        if let Some(native) = Self::convert_assistant_native_blocks(trimmed) {
                            return native;
                        }
                    }
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&m.content) {
                        if let Some(tool_calls_value) = value.get("tool_calls") {
                            if let Ok(parsed_calls) =
                                serde_json::from_value::<Vec<ProviderToolCall>>(
                                    tool_calls_value.clone(),
                                )
                            {
                                let tool_calls = parsed_calls
                                    .into_iter()
                                    .map(|tc| NativeToolCall {
                                        id: Some(
                                            crate::providers::sanitize::normalize_tool_call_id_for_provider(
                                                Some(tc.id),
                                                crate::providers::sanitize::ProviderKind::OpenAi,
                                            ),
                                        ),
                                        kind: Some("function".to_string()),
                                        function: NativeFunctionCall {
                                            name: tc.name,
                                            arguments: tc.arguments,
                                        },
                                    })
                                    .collect::<Vec<_>>();
                                let content = value
                                    .get("content")
                                    .and_then(serde_json::Value::as_str)
                                    .map(ToString::to_string);
                                let reasoning_content = value
                                    .get("reasoning_content")
                                    .and_then(serde_json::Value::as_str)
                                    .map(ToString::to_string);
                                let tool_calls = if tool_calls.is_empty() {
                                    None
                                } else {
                                    Some(tool_calls)
                                };
                                return NativeMessage {
                                    role: "assistant".to_string(),
                                    content,
                                    tool_call_id: None,
                                    tool_calls,
                                    reasoning_content,
                                };
                            }
                        }
                    }
                }

                if m.role == "tool" {
                    let trimmed = m.content.trim_start();
                    if trimmed.starts_with('[') {
                        if let Some(native) = Self::convert_tool_result_native_blocks(trimmed) {
                            return native;
                        }
                    }
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&m.content) {
                        let tool_call_id = value
                            .get("tool_call_id")
                            .and_then(serde_json::Value::as_str)
                            .or_else(|| {
                                value.get("tool_use_id").and_then(serde_json::Value::as_str)
                            })
                            .map(ToString::to_string);
                        let content = value
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string);
                        return NativeMessage {
                            role: "tool".to_string(),
                            content,
                            tool_call_id,
                            tool_calls: None,
                            reasoning_content: None,
                        };
                    }
                }

                NativeMessage {
                    role: m.role.clone(),
                    content: Some(m.content.clone()),
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                }
            })
            .collect()
    }

    fn convert_assistant_native_blocks(content: &str) -> Option<NativeMessage> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(content).ok()?;
        let mut text_parts: Vec<String> = Vec::new();
        let mut reasoning_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<NativeToolCall> = Vec::new();

        for item in arr {
            let kind = match item.get("type").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => continue,
            };
            match kind {
                "text" => {
                    if let Some(s) = item.get("text").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            text_parts.push(s.to_string());
                        }
                    }
                }
                "thinking" => {
                    if let Some(s) = item.get("thinking").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            reasoning_parts.push(s.to_string());
                        }
                    }
                }
                "tool_use" => {
                    let raw_id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .or_else(|| item.get("tool_use_id").and_then(|v| v.as_str()))
                        .or_else(|| item.get("tool_call_id").and_then(|v| v.as_str()))
                        .map(str::to_string);
                    let name = match item.get("name").and_then(|v| v.as_str()) {
                        Some(n) if !n.is_empty() => n.to_string(),
                        _ => continue,
                    };
                    let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                        raw_id,
                        crate::providers::sanitize::ProviderKind::OpenAi,
                    );
                    let input_val = item.get("input").cloned().unwrap_or_else(|| {
                        serde_json::Value::Object(serde_json::Map::new())
                    });
                    let arguments = serde_json::to_string(&input_val)
                        .unwrap_or_else(|_| "{}".to_string());
                    tool_calls.push(NativeToolCall {
                        id: Some(id),
                        kind: Some("function".to_string()),
                        function: NativeFunctionCall { name, arguments },
                    });
                }
                _ => {}
            }
        }

        if tool_calls.is_empty() && text_parts.is_empty() && reasoning_parts.is_empty() {
            return None;
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };
        let reasoning_content = if reasoning_parts.is_empty() {
            None
        } else {
            Some(reasoning_parts.join("\n"))
        };
        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        Some(NativeMessage {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_calls,
            reasoning_content,
        })
    }

    fn convert_tool_result_native_blocks(content: &str) -> Option<NativeMessage> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(content).ok()?;
        let mut tool_call_id: Option<String> = None;
        let mut body_parts: Vec<String> = Vec::new();
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                continue;
            }
            if tool_call_id.is_none() {
                tool_call_id = item
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("tool_call_id").and_then(|v| v.as_str()))
                    .map(str::to_string);
            }
            let part = match item.get("content") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            if !part.is_empty() {
                body_parts.push(part);
            }
        }
        let id = tool_call_id?;
        Some(NativeMessage {
            role: "tool".to_string(),
            content: Some(body_parts.join("\n")),
            tool_call_id: Some(id),
            tool_calls: None,
            reasoning_content: None,
        })
    }

    fn parse_native_response(message: NativeResponseMessage) -> ProviderChatResponse {
        let text = message.effective_content();
        let reasoning_content = message.reasoning_content.clone();
        let tool_calls = message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let arguments = crate::providers::sanitize::normalize_tool_call_arguments(
                    &tc.function.name,
                    tc.function.arguments,
                );
                ProviderToolCall {
                    id: crate::providers::sanitize::normalize_tool_call_id_for_provider(
                        tc.id,
                        crate::providers::sanitize::ProviderKind::OpenAi,
                    ),
                    name: tc.function.name,
                    arguments,
                }
            })
            .collect::<Vec<_>>();

        ProviderChatResponse {
            text,
            tool_calls,
            usage: None,
            reasoning_content,
        }
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts_and_headers(
                "provider.openai",
                self.timeout_secs,
                10,
                &self.extra_headers,
            )
    }

    fn stream_http_client(&self) -> Client {
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, value) in &self.extra_headers {
            match (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                (Ok(name), Ok(val)) => {
                    headers.insert(name, val);
                }
                _ => {
                    tracing::warn!(header = key, "Skipping invalid extra header name or value");
                }
            }
        }

        let read_timeout_secs = self.timeout_secs.max(300);
        crate::services::require_services()
            .proxy_runtime()
            .build_stream_client("provider.openai.stream", read_timeout_secs, 10, &headers)
    }

    fn stream_chunks_for_messages(
        &self,
        messages: Vec<Message>,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let credential = match self.credential.as_ref() {
            Some(value) => value.clone(),
            None => {
                return stream::once(async {
                    Err(StreamError::Provider(
                        "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml."
                            .to_string(),
                    ))
                })
                .boxed();
            }
        };

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            max_tokens: Self::max_tokens_field(self.max_tokens, model),
            max_completion_tokens: Self::max_completion_tokens_field(self.max_tokens, model),
            stream: Some(options.enabled),
            stream_options: options
                .enabled
                .then_some(StreamOptionsField { include_usage: true }),
        };

        let url = format!("{}/chat/completions", self.base_url);
        let client = self.stream_http_client();
        let count_tokens = options.count_tokens;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let _ = crate::runtime::spawn_supervised("providers.openai.stream_chunks", async move {
            let response = match client
                .post(&url)
                .header("Authorization", format!("Bearer {credential}"))
                .header("Accept", "text/event-stream")
                .json(&request)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(StreamError::Http(e))).await;
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let error_body = match response.text().await {
                    Ok(text) => text,
                    Err(_) => format!("HTTP error: {status}"),
                };
                let sanitized = super::super::sanitize_api_error(&error_body);
                let _ = tx
                    .send(Err(StreamError::Provider(format!("{status}: {sanitized}"))))
                    .await;
                return;
            }

            let mut chunk_stream =
                crate::providers::core::openai_sse::sse_bytes_to_chunks(response, count_tokens);
            while let Some(chunk) = chunk_stream.next().await {
                if tx.send(chunk).await.is_err() {
                    break;
                }
            }
        });

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        })
        .boxed()
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);

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
            temperature: adjusted_temperature,
            max_tokens: Self::max_tokens_field(self.max_tokens, model),
            max_completion_tokens: Self::max_completion_tokens_field(self.max_tokens, model),
            stream: None,
            stream_options: None,
        };

        let response = self
            .http_client()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI", response).await);
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.effective_content())
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);

        let sanitized_messages =
            crate::providers::sanitize::sanitize_messages_before_send_for_provider(
                request.messages.to_vec(),
                model,
                self.max_tokens.unwrap_or(0) as usize,
                None,
                crate::providers::sanitize::ProviderKind::OpenAi,
            );
        let tools = Self::convert_tools(request.tools);
        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(&sanitized_messages),
            temperature: adjusted_temperature,
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: Self::max_tokens_field(self.max_tokens, model),
            max_completion_tokens: Self::max_completion_tokens_field(self.max_tokens, model),
            stream: None,
            stream_options: None,
        };

        let response = self
            .http_client()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI", response).await);
        }

        let native_response: NativeChatResponse = response.json().await?;
        let usage = native_response.usage.map(|u| {
            let cached = u.prompt_tokens_details.and_then(|d| d.cached_tokens);
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, None);
            TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: None,
            }
        });
        let message = native_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))?;
        let mut result = Self::parse_native_response(message);
        result.usage = usage;
        Ok(result)
    }

    fn supports_native_tools(&self) -> bool {
        true
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);

        let sanitized_messages =
            crate::providers::sanitize::sanitize_messages_before_send_for_provider(
                messages.to_vec(),
                model,
                self.max_tokens.unwrap_or(0) as usize,
                None,
                crate::providers::sanitize::ProviderKind::OpenAi,
            );

        let native_tools: Option<Vec<NativeToolSpec>> = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .cloned()
                    .map(parse_native_tool_spec)
                    .collect::<Result<Vec<_>, _>>()?,
            )
        };

        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(&sanitized_messages),
            temperature: adjusted_temperature,
            tool_choice: native_tools.as_ref().map(|_| "auto".to_string()),
            tools: native_tools,
            max_tokens: Self::max_tokens_field(self.max_tokens, model),
            max_completion_tokens: Self::max_completion_tokens_field(self.max_tokens, model),
            stream: None,
            stream_options: None,
        };

        let response = self
            .http_client()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI", response).await);
        }

        let native_response: NativeChatResponse = response.json().await?;
        let usage = native_response.usage.map(|u| {
            let cached = u.prompt_tokens_details.and_then(|d| d.cached_tokens);
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, None);
            TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: None,
            }
        });
        let message = native_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))?;
        let mut result = Self::parse_native_response(message);
        result.usage = usage;
        Ok(result)
    }

    async fn chat_structured(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<StructuredResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);

        let response_format = serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "structured_output",
                "schema": schema,
                "strict": true
            }
        });

        let body = serde_json::json!({
            "model": model,
            "messages": Self::convert_messages(messages),
            "temperature": adjusted_temperature,
            "response_format": response_format,
            "max_tokens": self.max_tokens,
        });

        let response = self
            .http_client()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {credential}"))
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI", response).await);
        }

        let native_response: NativeChatResponse = response.json().await?;
        let usage = native_response.usage.map(|u| {
            let cached = u.prompt_tokens_details.and_then(|d| d.cached_tokens);
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, None);
            TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: None,
            }
        });
        let raw = native_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.effective_content())
            .unwrap_or_default();
        let parsed = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .or_else(|| parse_first_json_object(&raw))
            .ok_or_else(|| {
                anyhow::anyhow!("OpenAI structured chat returned non-JSON payload: {raw}")
            })?;
        Ok(StructuredResponse {
            data: parsed,
            raw_text: raw,
            usage,
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_streaming_tool_events(&self) -> bool {
        true
    }

    fn stream_chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
        let credential = match self.credential.as_ref() {
            Some(value) => value.clone(),
            None => {
                return stream::once(async {
                    Err(StreamError::Provider(
                        "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml."
                            .to_string(),
                    ))
                })
                .boxed();
            }
        };

        let adjusted_temperature = Self::adjust_temperature_for_model(model, temperature);
        let sanitized_messages =
            crate::providers::sanitize::sanitize_messages_before_send_for_provider(
                request.messages.to_vec(),
                model,
                self.max_tokens.unwrap_or(0) as usize,
                None,
                crate::providers::sanitize::ProviderKind::OpenAi,
            );
        let tools = Self::convert_tools(request.tools);
        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(&sanitized_messages),
            temperature: adjusted_temperature,
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: Self::max_tokens_field(self.max_tokens, model),
            max_completion_tokens: Self::max_completion_tokens_field(self.max_tokens, model),
            stream: Some(options.enabled),
            stream_options: options
                .enabled
                .then_some(StreamOptionsField { include_usage: true }),
        };

        let url = format!("{}/chat/completions", self.base_url);
        let client = self.stream_http_client();
        let count_tokens = options.count_tokens;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

        let _ = crate::runtime::spawn_supervised("providers.openai.stream_chat", async move {
            let response = match client
                .post(&url)
                .header("Authorization", format!("Bearer {credential}"))
                .header("Accept", "text/event-stream")
                .json(&native_request)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(StreamError::Http(e))).await;
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let error_body = match response.text().await {
                    Ok(text) => text,
                    Err(_) => format!("HTTP error: {status}"),
                };
                let sanitized = super::super::sanitize_api_error(&error_body);
                let _ = tx
                    .send(Err(StreamError::Provider(format!("{status}: {sanitized}"))))
                    .await;
                return;
            }

            let mut event_stream =
                crate::providers::core::openai_sse::sse_bytes_to_events(response, count_tokens);
            while let Some(event) = event_stream.next().await {
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })
        .boxed()
    }

    fn stream_chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
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

        self.stream_chunks_for_messages(messages, model, temperature, options)
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let sanitized = super::super::traits::flatten_messages_for_text_only_wire(messages);
        let api_messages: Vec<Message> = sanitized
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        self.stream_chunks_for_messages(api_messages, model, temperature, options)
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
