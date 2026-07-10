// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::multimodal;
use crate::providers::core::openai_sse::{sse_bytes_to_chunks, sse_bytes_to_events};
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, StreamChunk, StreamError, StreamEvent, StreamOptions,
    StreamResult, TokenUsage, ToolCall as ProviderToolCall,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct OpenRouterProvider {
    credential: Option<String>,
    timeout_secs: u64,
    max_tokens: Option<u32>,
    model_context_windows: std::collections::HashMap<String, u32>,
    extra_headers: std::collections::HashMap<String, String>,
}

const DEFAULT_OPENROUTER_TIMEOUT_SECS: u64 = 120;
const OPENROUTER_CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: MessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Parts(Vec<MessagePart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MessagePart {
    Text { text: String },
    ImageUrl { image_url: ImageUrlPart },
}

#[derive(Debug, Serialize)]
struct ImageUrlPart {
    url: String,
}

#[derive(Debug, Deserialize)]
struct ApiChatResponse {
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
    stream: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptionsField>,

    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct StreamOptionsField {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "crate::providers::sanitize::skip_serializing_tool_calls")]
    tool_calls: Option<Vec<NativeToolCall>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize)]
struct NativeToolSpec {
    #[serde(rename = "type")]
    kind: String,
    function: NativeToolFunctionSpec,
}

#[derive(Debug, Serialize)]
struct NativeToolFunctionSpec {
    name: String,
    description: String,
    parameters: serde_json::Value,
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
}

#[derive(Debug, Deserialize)]
struct NativeChoice {
    message: NativeResponseMessage,
}

#[derive(Debug, Deserialize)]
struct NativeResponseMessage {
    #[serde(default)]
    content: Option<String>,

    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<NativeToolCall>>,
}

impl OpenRouterProvider {
    pub fn new(credential: Option<&str>, timeout_secs: Option<u64>) -> Self {
        Self {
            credential: credential.map(ToString::to_string),
            timeout_secs: timeout_secs
                .filter(|secs| *secs > 0)
                .unwrap_or(DEFAULT_OPENROUTER_TIMEOUT_SECS),
            max_tokens: None,
            model_context_windows: std::collections::HashMap::new(),
            extra_headers: std::collections::HashMap::new(),
        }
    }

    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_model_context_windows(
        mut self,
        windows: std::collections::HashMap<String, u32>,
    ) -> Self {
        self.model_context_windows = windows;
        self
    }

    pub fn with_extra_headers(
        mut self,
        headers: std::collections::HashMap<String, String>,
    ) -> Self {
        self.extra_headers = headers;
        self
    }

    fn context_window_for(&self, model: &str) -> usize {
        if let Some(value) = self.model_context_windows.get(model).copied() {
            return value as usize;
        }
        let id = model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();
        if let Some(value) = self.model_context_windows.get(id.as_str()).copied() {
            return value as usize;
        }
        crate::constants::api_limits::context_window_for_model(model) as usize
    }

    fn reserved_output_tokens(&self, model: &str) -> usize {
        let window = self.context_window_for(model);
        let configured = self.max_tokens.map(|v| v as usize);
        let default_reserve = (window / 8).clamp(512, 4096);
        let raw = configured.unwrap_or(default_reserve);
        let max_reserve = window.saturating_sub(512).max(512);
        raw.clamp(256, max_reserve)
    }

    fn adjust_temperature_for_model(model: &str, requested: f64) -> f64 {
        if Self::model_requires_unit_temperature(model) {
            1.0
        } else {
            requested
        }
    }

    fn model_requires_unit_temperature(model: &str) -> bool {
        let id = model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();
        id.starts_with("kimi-k2") || id.starts_with("kimi-thinking")
    }

    fn reasoning_param_for_model(&self, model: &str) -> Option<serde_json::Value> {
        if Self::is_reasoning_blacklisted(model) {
            return None;
        }
        Some(serde_json::json!({
            "enabled": true,
            "effort": "high",
        }))
    }

    fn reasoning_blacklist_key(model: &str) -> String {
        format!("openrouter::{}", model.to_ascii_lowercase())
    }

    fn is_reasoning_blacklisted(model: &str) -> bool {
        let key = Self::reasoning_blacklist_key(model);
        let store = reasoning_blacklist_store();
        store
            .read()
            .map(|set| set.contains(&key))
            .unwrap_or(false)
    }

    fn blacklist_reasoning(model: &str) {
        let key = Self::reasoning_blacklist_key(model);
        let store = reasoning_blacklist_store();
        if let Ok(mut set) = store.write() {
            set.insert(key);
        }
    }

    fn is_reasoning_param_unsupported(status: reqwest::StatusCode, error: &str) -> bool {
        if !matches!(
            status,
            reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY
        ) {
            return false;
        }
        let lower = error.to_lowercase();
        let mentions = lower.contains("reasoning") || lower.contains("thinking");
        if !mentions {
            return false;
        }
        [
            "unknown parameter",
            "unsupported parameter",
            "unrecognized field",
            "unrecognized parameter",
            "unknown field",
            "invalid parameter",
            "invalid field",
            "not supported",
            "does not support",
            "extra field",
            "extra fields not permitted",
            "unexpected field",
            "unexpected parameter",
            "additional properties",
            "no additional properties",
        ]
        .iter()
        .any(|hint| lower.contains(hint))
    }

    fn model_supports_native_tools(model: &str) -> bool {
        let id = model.to_ascii_lowercase();

        let denylist_substrings: [&str; 8] = [
            "moonshotai/moonshot-v1",
            "moonshot-v1-8k",
            "moonshot-v1-32k",
            "moonshot-v1-128k",
            "moonshot-v1-auto",
            "qwen-72b",
            "-instruct-v0.1",
            "-instruct-v0.2",
        ];

        !denylist_substrings.iter().any(|p| id.contains(p))
    }

    fn is_native_tool_schema_unsupported(status: reqwest::StatusCode, error: &str) -> bool {
        if !matches!(
            status,
            reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY
        ) {
            return false;
        }
        let lower = error.to_lowercase();
        [
            "unknown parameter: tools",
            "unsupported parameter: tools",
            "unrecognized field `tools`",
            "does not support tools",
            "function calling is not supported",
            "tool_choice",
            "tool call validation failed",
            "was not in request",
            "tokenization failed",
            "invalid request: tokenization",
            "tokenizer error",
            "tokenizer failed",
            "invalid tools",
            "invalid tool schema",
            "invalid function schema",
            "invalid `tools`",
            "invalid 'tools'",
            "tool definition invalid",
            "tools schema",
            "function schema",
            "json schema validation failed",
            "schema validation failed",
            "invalid messages",
            "invalid `messages`",
            "messages content type",
            "content must be a string",
            "image_url is not supported",
            "vision is not supported",
            "multimodal not supported",
        ]
        .iter()
        .any(|hint| lower.contains(hint))
    }

    fn with_prompt_guided_tool_instructions(
        messages: &[ChatMessage],
        tools: Option<&[ToolSpec]>,
    ) -> Vec<ChatMessage> {
        let Some(tools) = tools else {
            return messages.to_vec();
        };
        if tools.is_empty() {
            return messages.to_vec();
        }
        let instructions = crate::providers::traits::build_tool_instructions_text(tools);
        let mut modified = messages.to_vec();
        if let Some(sys) = modified.iter_mut().find(|m| m.role == "system") {
            if !sys.content.is_empty() {
                sys.content.push_str("\n\n");
            }
            sys.content.push_str(&instructions);
        } else {
            modified.insert(0, ChatMessage::system(instructions));
        }
        modified
    }

    async fn api_error_text(response: reqwest::Response) -> (reqwest::StatusCode, String) {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        (status, body)
    }

    fn convert_tools(tools: Option<&[ToolSpec]>) -> Option<Vec<NativeToolSpec>> {
        let items = tools?;
        if items.is_empty() {
            return None;
        }
        let deduped = crate::tools::dedupe_tool_specs(items);
        let valid: Vec<NativeToolSpec> = deduped
            .iter()
            .filter(|tool| is_valid_openai_tool_name(&tool.name))
            .map(|tool| NativeToolSpec {
                kind: "function".to_string(),
                function: NativeToolFunctionSpec {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                },
            })
            .collect();
        if valid.is_empty() { None } else { Some(valid) }
    }

    fn convert_messages(messages: &[ChatMessage]) -> Vec<NativeMessage> {
        messages
            .iter()
            .map(|m| {
                if m.role == "assistant" {
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
                                        id: Some(crate::providers::sanitize::normalize_tool_call_id(
                                            Some(tc.id),
                                        )),
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
                                    .map(|value| MessageContent::Text(value.to_string()));
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
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&m.content) {
                        let tool_call_id = value
                            .get("tool_call_id")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string);
                        let content = value
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .map(|value| MessageContent::Text(value.to_string()))
                            .or_else(|| Some(MessageContent::Text(m.content.clone())));
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
                    content: Some(Self::to_message_content(&m.role, &m.content)),
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                }
            })
            .collect()
    }

    fn to_message_content(role: &str, content: &str) -> MessageContent {
        if role != "user" {
            return MessageContent::Text(content.to_string());
        }

        let (cleaned_text, image_refs) = multimodal::parse_image_markers(content);
        if image_refs.is_empty() {
            return MessageContent::Text(content.to_string());
        }

        let mut parts = Vec::with_capacity(image_refs.len() + 1);
        let trimmed_text = cleaned_text.trim();
        if !trimmed_text.is_empty() {
            parts.push(MessagePart::Text {
                text: trimmed_text.to_string(),
            });
        }

        for image_ref in image_refs {
            parts.push(MessagePart::ImageUrl {
                image_url: ImageUrlPart { url: image_ref },
            });
        }

        MessageContent::Parts(parts)
    }

    fn parse_native_response(message: NativeResponseMessage) -> ProviderChatResponse {
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
                    id: crate::providers::sanitize::normalize_tool_call_id(tc.id),
                    name: tc.function.name,
                    arguments,
                }
            })
            .collect::<Vec<_>>();

        ProviderChatResponse {
            text: message.content,
            tool_calls,
            usage: None,
            reasoning_content,
        }
    }

    fn compact_sanitized_body_snippet(body: &str) -> String {
        super::sanitize_api_error(body)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    async fn read_response_body(
        provider_name: &str,
        response: reqwest::Response,
    ) -> anyhow::Result<String> {
        response.text().await.map_err(|error| {
            let sanitized = super::sanitize_api_error(&error.to_string());
            anyhow::anyhow!(
                "{provider_name} transport error while reading response body: {sanitized}"
            )
        })
    }

    fn parse_response_body<T: DeserializeOwned>(
        provider_name: &str,
        body: &str,
        kind: &str,
    ) -> anyhow::Result<T> {
        serde_json::from_str::<T>(body).map_err(|error| {
            let snippet = Self::compact_sanitized_body_snippet(body);
            anyhow::anyhow!(
                "{provider_name} API returned an unexpected {kind} payload: {error}; body={snippet}"
            )
        })
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts_and_headers(
                "provider.openrouter",
                self.timeout_secs,
                OPENROUTER_CONNECT_TIMEOUT_SECS,
                &self.extra_headers,
            )
    }

    fn stream_http_client(&self) -> Client {
        let read_timeout_secs = self.timeout_secs.max(300);

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

        crate::services::require_services()
            .proxy_runtime()
            .build_stream_client(
                "provider.openrouter.stream",
                read_timeout_secs,
                OPENROUTER_CONNECT_TIMEOUT_SECS,
                &headers,
            )
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
            prompt_caching: false,
            responses_api: false,
        }
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_streaming_tool_events(&self) -> bool {

        true
    }

    async fn warmup(&self) -> anyhow::Result<()> {

        if let Some(credential) = self.credential.as_ref() {
            self.http_client()
                .get("https://openrouter.ai/api/v1/auth/key")
                .header("Authorization", format!("Bearer {credential}"))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
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
                "OpenRouter API key not set. Run `sen onboard` or set OPENROUTER_API_KEY env var."
            )
        })?;

        let mut messages = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system".to_string(),
                content: MessageContent::Text(sys.to_string()),
            });
        }

        messages.push(Message {
            role: "user".to_string(),
            content: Self::to_message_content("user", message),
        });

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            max_tokens: self.max_tokens,
            response_format: None,
            stream: None,
            reasoning: self.reasoning_param_for_model(model),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header(
                "HTTP-Referer",
                "https://github.com/senweaver/SenWeaverCoding",
            )
            .header("X-Title", "SenWeaverCoding")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let (status, body) = Self::api_error_text(response).await;
            let sanitized = super::sanitize_api_error(&body);
            if !Self::is_reasoning_blacklisted(model)
                && Self::is_reasoning_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.openrouter",
                    model,
                    status = %status,
                    "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and retrying without reasoning"
                );
                Self::blacklist_reasoning(model);
                return Box::pin(self.chat_with_system(system_prompt, message, model, temperature))
                    .await;
            }
            anyhow::bail!("OpenRouter API error ({status}): {sanitized}");
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let chat_response =
            Self::parse_response_body::<ApiChatResponse>("OpenRouter", &body, "chat-completions")?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.unwrap_or_default())
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenRouter API key not set. Run `sen onboard` or set OPENROUTER_API_KEY env var."
            )
        })?;

        let sanitized = super::traits::flatten_messages_for_text_only_wire(messages);
        let budgeted = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            sanitized,
            model,
            self.reserved_output_tokens(model),
            Some(self.context_window_for(model)),
        );
        let api_messages: Vec<Message> = budgeted
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(&m.role, &m.content),
            })
            .collect();

        let request = ChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            max_tokens: self.max_tokens,
            response_format: None,
            stream: None,
            reasoning: self.reasoning_param_for_model(model),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header(
                "HTTP-Referer",
                "https://github.com/senweaver/SenWeaverCoding",
            )
            .header("X-Title", "SenWeaverCoding")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let (status, body) = Self::api_error_text(response).await;
            let sanitized = super::sanitize_api_error(&body);
            if !Self::is_reasoning_blacklisted(model)
                && Self::is_reasoning_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.openrouter",
                    model,
                    status = %status,
                    "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and retrying without reasoning"
                );
                Self::blacklist_reasoning(model);
                return Box::pin(self.chat_with_history(messages, model, temperature)).await;
            }
            anyhow::bail!("OpenRouter API error ({status}): {sanitized}");
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let chat_response =
            Self::parse_response_body::<ApiChatResponse>("OpenRouter", &body, "chat-completions")?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.unwrap_or_default())
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenRouter API key not set. Run `sen onboard` or set OPENROUTER_API_KEY env var."
            )
        })?;

        let model_supports_native = Self::model_supports_native_tools(model);
        let has_tools = request.tools.is_some_and(|t| !t.is_empty());
        let allow_native_tools = has_tools && model_supports_native;

        if !model_supports_native {
            tracing::debug!(
                target: "providers.openrouter",
                model,
                has_tools,
                "model is on the legacy/no-tools allowlist; routing chat() through chat_with_history"
            );
            let guided = if has_tools {
                Self::with_prompt_guided_tool_instructions(request.messages, request.tools)
            } else {
                request.messages.to_vec()
            };
            let text = self
                .chat_with_history(&guided, model, temperature)
                .await?;
            return Ok(ProviderChatResponse::text_only(Some(text), None));
        }

        let tools = if allow_native_tools {
            Self::convert_tools(request.tools)
        } else {
            None
        };
        let budgeted_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            self.reserved_output_tokens(model),
            Some(self.context_window_for(model)),
        );
        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(&budgeted_messages),
            temperature: Self::adjust_temperature_for_model(model, temperature),
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: self.max_tokens,
            stream: None,
            stream_options: None,
            reasoning: self.reasoning_param_for_model(model),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header(
                "HTTP-Referer",
                "https://github.com/senweaver/SenWeaverCoding",
            )
            .header("X-Title", "SenWeaverCoding")
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let (status, body) = Self::api_error_text(response).await;
            let sanitized = super::sanitize_api_error(&body);
            if !Self::is_reasoning_blacklisted(model)
                && Self::is_reasoning_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.openrouter",
                    model,
                    status = %status,
                    "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and retrying without reasoning"
                );
                Self::blacklist_reasoning(model);
                return Box::pin(self.chat(request, model, temperature)).await;
            }
            if Self::is_native_tool_schema_unsupported(status, &sanitized) {
                tracing::warn!(
                    target: "providers.openrouter",
                    model,
                    status = %status,
                    "OpenRouter rejected native tools ({sanitized}); retrying via chat_with_history"
                );
                let guided = Self::with_prompt_guided_tool_instructions(
                    request.messages,
                    request.tools,
                );
                let text = self
                    .chat_with_history(&guided, model, temperature)
                    .await?;
                return Ok(ProviderChatResponse::text_only(Some(text), None));
            }
            anyhow::bail!("OpenRouter API error ({status}): {sanitized}");
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let native_response =
            Self::parse_response_body::<NativeChatResponse>("OpenRouter", &body, "native chat")?;
        let usage = native_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
        });
        let message = native_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))?;
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
            anyhow::anyhow!(
                "OpenRouter API key not set. Run `sen onboard` or set OPENROUTER_API_KEY env var."
            )
        })?;

        let model_supports_native = Self::model_supports_native_tools(model);
        let has_tools = !tools.is_empty();
        let allow_native_tools = has_tools && model_supports_native;

        if !model_supports_native {
            tracing::debug!(
                target: "providers.openrouter",
                model,
                has_tools,
                "model is on the legacy/no-tools allowlist; chat_with_tools routing through chat_with_history"
            );
            let text = self.chat_with_history(messages, model, temperature).await?;
            return Ok(ProviderChatResponse::text_only(Some(text), None));
        }

        let native_tools: Option<Vec<NativeToolSpec>> = if !allow_native_tools {
            None
        } else {
            let specs: Vec<NativeToolSpec> = tools
                .iter()
                .filter_map(|t| {
                    let func = t.get("function")?;
                    Some(NativeToolSpec {
                        kind: "function".to_string(),
                        function: NativeToolFunctionSpec {
                            name: func.get("name")?.as_str()?.to_string(),
                            description: func
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string(),
                            parameters: func
                                .get("parameters")
                                .cloned()
                                .unwrap_or(serde_json::json!({})),
                        },
                    })
                })
                .collect();
            if specs.is_empty() { None } else { Some(specs) }
        };

        let budgeted_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            model,
            self.reserved_output_tokens(model),
            Some(self.context_window_for(model)),
        );
        let native_messages = Self::convert_messages(&budgeted_messages);

        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: native_messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            tool_choice: native_tools.as_ref().map(|_| "auto".to_string()),
            tools: native_tools,
            max_tokens: self.max_tokens,
            stream: None,
            stream_options: None,
            reasoning: self.reasoning_param_for_model(model),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header(
                "HTTP-Referer",
                "https://github.com/senweaver/SenWeaverCoding",
            )
            .header("X-Title", "SenWeaverCoding")
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let (status, body) = Self::api_error_text(response).await;
            let sanitized = super::sanitize_api_error(&body);
            if !Self::is_reasoning_blacklisted(model)
                && Self::is_reasoning_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.openrouter",
                    model,
                    status = %status,
                    "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and retrying without reasoning"
                );
                Self::blacklist_reasoning(model);
                return Box::pin(self.chat_with_tools(messages, tools, model, temperature)).await;
            }
            if Self::is_native_tool_schema_unsupported(status, &sanitized) {
                tracing::warn!(
                    target: "providers.openrouter",
                    model,
                    status = %status,
                    "OpenRouter rejected native tools ({sanitized}); retrying via chat_with_history"
                );
                let text = self.chat_with_history(messages, model, temperature).await?;
                return Ok(ProviderChatResponse::text_only(Some(text), None));
            }
            anyhow::bail!("OpenRouter API error ({status}): {sanitized}");
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let native_response =
            Self::parse_response_body::<NativeChatResponse>("OpenRouter", &body, "native chat")?;
        let usage = native_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
        });
        let message = native_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))?;
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
    ) -> anyhow::Result<crate::providers::traits::StructuredResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenRouter API key not set. Run `sen onboard` or set OPENROUTER_API_KEY env var."
            )
        })?;

        let api_messages: Vec<Message> = messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(&m.role, &m.content),
            })
            .collect();

        let response_format = serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "structured_output",
                "schema": schema,
                "strict": true
            }
        });

        let request = ChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            max_tokens: self.max_tokens,
            response_format: Some(response_format),
            stream: None,
            reasoning: self.reasoning_param_for_model(model),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header(
                "HTTP-Referer",
                "https://github.com/senweaver/SenWeaverCoding",
            )
            .header("X-Title", "SenWeaverCoding")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let (status, body) = Self::api_error_text(response).await;
            let sanitized = super::sanitize_api_error(&body);
            if !Self::is_reasoning_blacklisted(model)
                && Self::is_reasoning_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.openrouter",
                    model,
                    status = %status,
                    "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and retrying without reasoning"
                );
                Self::blacklist_reasoning(model);
                return Box::pin(self.chat_structured(messages, schema, model, temperature)).await;
            }
            anyhow::bail!("OpenRouter API error ({status}): {sanitized}");
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let chat_response = Self::parse_response_body::<ApiChatResponse>(
            "OpenRouter",
            &body,
            "structured chat-completions",
        )?;
        let raw_text = chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.unwrap_or_default())
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))?;

        let parsed = serde_json::from_str::<serde_json::Value>(&raw_text)
            .ok()
            .or_else(|| crate::providers::traits::parse_first_json_object(&raw_text))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "OpenRouter structured-output reply was not valid JSON: {raw_text}"
                )
            })?;

        Ok(crate::providers::traits::StructuredResponse {
            data: parsed,
            raw_text,
            usage: None,
        })
    }

    fn stream_chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
        if !options.enabled {
            return stream::once(async { Ok(StreamEvent::Final) }).boxed();
        }

        let credential = match self.credential.as_ref() {
            Some(value) => value.clone(),
            None => {
                return stream::once(async move {
                    Err(StreamError::Provider(
                        "OpenRouter API key not set. Run `sen onboard` or set \
                         OPENROUTER_API_KEY env var.".to_string(),
                    ))
                })
                .boxed();
            }
        };

        let tools = Self::convert_tools(request.tools);
        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            self.reserved_output_tokens(model),
            Some(self.context_window_for(model)),
        );
        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(&sanitized_messages),
            temperature: Self::adjust_temperature_for_model(model, temperature),
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: self.max_tokens,
            stream: Some(true),
            stream_options: Some(StreamOptionsField { include_usage: true }),
            reasoning: self.reasoning_param_for_model(model),
        };

        let payload = match serde_json::to_value(&native_request) {
            Ok(value) => value,
            Err(error) => {
                return stream::once(async move { Err(StreamError::Json(error)) }).boxed();
            }
        };

        let client = self.stream_http_client();
        let count_tokens = options.count_tokens;
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

        let provider_clone = self.clone();
        let model_owned = model.to_string();
        let temperature_owned = temperature;
        let options_owned = options;
        let retry_messages: Vec<ChatMessage> = request.messages.to_vec();
        let retry_tools: Option<Vec<ToolSpec>> = request.tools.map(|t| t.to_vec());

        let _ = crate::runtime::spawn_supervised(
            "providers.openrouter.stream_chat",
            async move {
                let response = client
                    .post("https://openrouter.ai/api/v1/chat/completions")
                    .header("Authorization", format!("Bearer {credential}"))
                    .header(
                        "HTTP-Referer",
                        "https://github.com/senweaver/SenWeaverCoding",
                    )
                    .header("X-Title", "SenWeaverCoding")
                    .header("Accept", "text/event-stream")
                    .json(&payload)
                    .send()
                    .await;

                let response = match response {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let error = response
                        .text()
                        .await
                        .unwrap_or_else(|_| format!("HTTP error: {status}"));
                    let sanitized = super::sanitize_api_error(&error);

                    if !Self::is_reasoning_blacklisted(&model_owned)
                        && Self::is_reasoning_param_unsupported(status, &sanitized)
                    {
                        tracing::warn!(
                            target: "providers.openrouter.stream",
                            model = %model_owned,
                            status = %status,
                            "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and re-issuing stream without reasoning"
                        );
                        Self::blacklist_reasoning(&model_owned);
                        let retry_request = crate::providers::traits::ChatRequest {
                            messages: retry_messages.as_slice(),
                            tools: retry_tools.as_deref(),
                        };
                        let mut retry_stream = provider_clone.stream_chat(
                            retry_request,
                            &model_owned,
                            temperature_owned,
                            options_owned,
                        );
                        while let Some(event) = retry_stream.next().await {
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                        return;
                    }

                    let _ = tx
                        .send(Err(StreamError::Provider(format!("{status}: {sanitized}"))))
                        .await;
                    return;
                }

                let mut event_stream = sse_bytes_to_events(response, count_tokens);
                while let Some(event) = event_stream.next().await {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
            },
        );

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
        let credential = match self.credential.as_ref() {
            Some(value) => value.clone(),
            None => {
                return stream::once(async move {
                    Err(StreamError::Provider(
                        "OpenRouter API key not set. Run `sen onboard` or set \
                         OPENROUTER_API_KEY env var.".to_string(),
                    ))
                })
                .boxed();
            }
        };

        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system".to_string(),
                content: MessageContent::Text(sys.to_string()),
            });
        }
        messages.push(Message {
            role: "user".to_string(),
            content: Self::to_message_content("user", message),
        });

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            max_tokens: self.max_tokens,
            response_format: None,
            stream: Some(options.enabled),
            reasoning: self.reasoning_param_for_model(model),
        };

        let client = self.stream_http_client();
        let count_tokens = options.count_tokens;
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let provider_clone = self.clone();
        let model_owned = model.to_string();
        let temperature_owned = temperature;
        let options_owned = options;
        let system_prompt_owned = system_prompt.map(ToString::to_string);
        let message_owned = message.to_string();

        let _ = crate::runtime::spawn_supervised(
            "providers.openrouter.stream_chat_with_system",
            async move {
                let response = client
                    .post("https://openrouter.ai/api/v1/chat/completions")
                    .header("Authorization", format!("Bearer {credential}"))
                    .header(
                        "HTTP-Referer",
                        "https://github.com/senweaver/SenWeaverCoding",
                    )
                    .header("X-Title", "SenWeaverCoding")
                    .header("Accept", "text/event-stream")
                    .json(&request)
                    .send()
                    .await;

                let response = match response {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let error = response
                        .text()
                        .await
                        .unwrap_or_else(|_| format!("HTTP error: {status}"));
                    let sanitized = super::sanitize_api_error(&error);

                    if !Self::is_reasoning_blacklisted(&model_owned)
                        && Self::is_reasoning_param_unsupported(status, &sanitized)
                    {
                        tracing::warn!(
                            target: "providers.openrouter.stream",
                            model = %model_owned,
                            status = %status,
                            "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and re-issuing stream without reasoning"
                        );
                        Self::blacklist_reasoning(&model_owned);
                        let mut retry_stream = provider_clone.stream_chat_with_system(
                            system_prompt_owned.as_deref(),
                            &message_owned,
                            &model_owned,
                            temperature_owned,
                            options_owned,
                        );
                        while let Some(chunk) = retry_stream.next().await {
                            if tx.send(chunk).await.is_err() {
                                break;
                            }
                        }
                        return;
                    }

                    let _ = tx
                        .send(Err(StreamError::Provider(format!("{status}: {sanitized}"))))
                        .await;
                    return;
                }

                let mut chunk_stream = sse_bytes_to_chunks(response, count_tokens);
                while let Some(chunk) = chunk_stream.next().await {
                    if tx.send(chunk).await.is_err() {
                        break;
                    }
                }
            },
        );

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        })
        .boxed()
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let credential = match self.credential.as_ref() {
            Some(value) => value.clone(),
            None => {
                return stream::once(async move {
                    Err(StreamError::Provider(
                        "OpenRouter API key not set. Run `sen onboard` or set \
                         OPENROUTER_API_KEY env var.".to_string(),
                    ))
                })
                .boxed();
            }
        };

        let sanitized = super::traits::flatten_messages_for_text_only_wire(messages);
        let budgeted = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            sanitized,
            model,
            self.reserved_output_tokens(model),
            Some(self.context_window_for(model)),
        );
        let api_messages: Vec<Message> = budgeted
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(&m.role, &m.content),
            })
            .collect();

        let request = ChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            max_tokens: self.max_tokens,
            response_format: None,
            stream: Some(options.enabled),
            reasoning: self.reasoning_param_for_model(model),
        };

        let client = self.stream_http_client();
        let count_tokens = options.count_tokens;
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let provider_clone = self.clone();
        let model_owned = model.to_string();
        let temperature_owned = temperature;
        let options_owned = options;
        let retry_messages: Vec<ChatMessage> = messages.to_vec();

        let _ = crate::runtime::spawn_supervised(
            "providers.openrouter.stream_chat_with_history",
            async move {
                let response = client
                    .post("https://openrouter.ai/api/v1/chat/completions")
                    .header("Authorization", format!("Bearer {credential}"))
                    .header(
                        "HTTP-Referer",
                        "https://github.com/senweaver/SenWeaverCoding",
                    )
                    .header("X-Title", "SenWeaverCoding")
                    .header("Accept", "text/event-stream")
                    .json(&request)
                    .send()
                    .await;

                let response = match response {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let error = response
                        .text()
                        .await
                        .unwrap_or_else(|_| format!("HTTP error: {status}"));
                    let sanitized = super::sanitize_api_error(&error);

                    if !Self::is_reasoning_blacklisted(&model_owned)
                        && Self::is_reasoning_param_unsupported(status, &sanitized)
                    {
                        tracing::warn!(
                            target: "providers.openrouter.stream",
                            model = %model_owned,
                            status = %status,
                            "reasoning parameter rejected by upstream ({sanitized}); blacklisting model and re-issuing stream without reasoning"
                        );
                        Self::blacklist_reasoning(&model_owned);
                        let mut retry_stream = provider_clone.stream_chat_with_history(
                            &retry_messages,
                            &model_owned,
                            temperature_owned,
                            options_owned,
                        );
                        while let Some(chunk) = retry_stream.next().await {
                            if tx.send(chunk).await.is_err() {
                                break;
                            }
                        }
                        return;
                    }

                    let _ = tx
                        .send(Err(StreamError::Provider(format!("{status}: {sanitized}"))))
                        .await;
                    return;
                }

                let mut chunk_stream = sse_bytes_to_chunks(response, count_tokens);
                while let Some(chunk) = chunk_stream.next().await {
                    if tx.send(chunk).await.is_err() {
                        break;
                    }
                }
            },
        );

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        })
        .boxed()
    }
}

fn is_valid_openai_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn reasoning_blacklist_store()
-> &'static std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>> {
    static STORE: std::sync::OnceLock<
        std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    > = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()))
    })
}
