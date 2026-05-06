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

pub struct OpenRouterProvider {
    credential: Option<String>,
    timeout_secs: u64,
    max_tokens: Option<u32>,
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
    content: String,
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
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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

    #[serde(default)]
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
                                        id: Some(tc.id),
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
                                return NativeMessage {
                                    role: "assistant".to_string(),
                                    content,
                                    tool_call_id: None,
                                    tool_calls: Some(tool_calls),
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
            .map(|tc| ProviderToolCall {
                id: tc.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                name: tc.function.name,
                arguments: tc.function.arguments,
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
        crate::config::build_runtime_proxy_client_with_timeouts(
            "provider.openrouter",
            self.timeout_secs,
            OPENROUTER_CONNECT_TIMEOUT_SECS,
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
            temperature,
            max_tokens: self.max_tokens,
            response_format: None,
            stream: None,
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
            return Err(super::api_error("OpenRouter", response).await);
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let chat_response =
            Self::parse_response_body::<ApiChatResponse>("OpenRouter", &body, "chat-completions")?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
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

        let api_messages: Vec<Message> = messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(&m.role, &m.content),
            })
            .collect();

        let request = ChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature,
            max_tokens: self.max_tokens,
            response_format: None,
            stream: None,
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
            return Err(super::api_error("OpenRouter", response).await);
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let chat_response =
            Self::parse_response_body::<ApiChatResponse>("OpenRouter", &body, "chat-completions")?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
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

        let tools = Self::convert_tools(request.tools);
        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(request.messages),
            temperature,
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: self.max_tokens,
            stream: None,
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
            return Err(super::api_error("OpenRouter", response).await);
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

        let native_tools: Option<Vec<NativeToolSpec>> = if tools.is_empty() {
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

        let native_messages = Self::convert_messages(messages);

        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: native_messages,
            temperature,
            tool_choice: native_tools.as_ref().map(|_| "auto".to_string()),
            tools: native_tools,
            max_tokens: self.max_tokens,
            stream: None,
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
            return Err(super::api_error("OpenRouter", response).await);
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
            temperature,
            max_tokens: self.max_tokens,
            response_format: Some(response_format),
            stream: None,
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
            return Err(super::api_error("OpenRouter", response).await);
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
            .map(|c| c.message.content)
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
        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(request.messages),
            temperature,
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: self.max_tokens,
            stream: Some(true),
        };

        let payload = match serde_json::to_value(&native_request) {
            Ok(value) => value,
            Err(error) => {
                return stream::once(async move { Err(StreamError::Json(error)) }).boxed();
            }
        };

        let client = self.http_client();
        let count_tokens = options.count_tokens;
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

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
                    let _ = tx
                        .send(Err(StreamError::Provider(format!("{status}: {error}"))))
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
            temperature,
            max_tokens: self.max_tokens,
            response_format: None,
            stream: Some(options.enabled),
        };

        let client = self.http_client();
        let count_tokens = options.count_tokens;
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

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
                    let _ = tx
                        .send(Err(StreamError::Provider(format!("{status}: {error}"))))
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

        let api_messages: Vec<Message> = messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(&m.role, &m.content),
            })
            .collect();

        let request = ChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature,
            max_tokens: self.max_tokens,
            response_format: None,
            stream: Some(options.enabled),
        };

        let client = self.http_client();
        let count_tokens = options.count_tokens;
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

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
                    let _ = tx
                        .send(Err(StreamError::Provider(format!("{status}: {error}"))))
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
