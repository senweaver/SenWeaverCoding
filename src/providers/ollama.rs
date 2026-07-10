// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::multimodal;
use crate::providers::traits::{
    ChatMessage, ChatResponse, Provider, ProviderCapabilities, TokenUsage, ToolCall,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub struct OllamaProvider {
    base_url: String,
    api_key: Option<String>,
    reasoning_enabled: Option<bool>,
    timeout_secs: u64,
    extra_headers: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    options: Options,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
struct Message {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "crate::providers::sanitize::skip_serializing_tool_calls")]
    tool_calls: Option<Vec<OutgoingToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct OutgoingToolCall {
    #[serde(rename = "type")]
    kind: String,
    function: OutgoingFunction,
}

#[derive(Debug, Clone, Serialize)]
struct OutgoingFunction {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct Options {
    temperature: f64,
}

#[derive(Debug, Deserialize)]
struct ApiChatResponse {
    message: ResponseMessage,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,

    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCall {
    id: Option<String>,
    function: OllamaFunction,
}

#[derive(Debug, Deserialize)]
struct OllamaFunction {
    name: String,
    #[serde(default, deserialize_with = "deserialize_args")]
    arguments: serde_json::Value,
}

fn deserialize_args<'de, D>(deserializer: D) -> Result<serde_json::Value, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    if let Some(s) = value.as_str() {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => Ok(v),
            Err(_) => Ok(serde_json::json!({})),
        }
    } else {
        Ok(value)
    }
}

impl OllamaProvider {
    fn normalize_base_url(raw_url: &str) -> String {
        let trimmed = raw_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return String::new();
        }

        trimmed
            .strip_suffix("/api/chat")
            .or_else(|| trimmed.strip_suffix("/api"))
            .unwrap_or(trimmed)
            .trim_end_matches('/')
            .to_string()
    }

    pub fn new(base_url: Option<&str>, api_key: Option<&str>) -> Self {
        Self::new_with_reasoning(base_url, api_key, None)
    }

    pub fn new_with_reasoning(
        base_url: Option<&str>,
        api_key: Option<&str>,
        reasoning_enabled: Option<bool>,
    ) -> Self {
        let api_key = api_key.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

        Self {
            base_url: Self::normalize_base_url(base_url.unwrap_or("http://localhost:11434")),
            api_key,
            reasoning_enabled,
            timeout_secs: 300,
            extra_headers: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs.max(1);
        self
    }

    #[must_use]
    pub fn with_extra_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    fn is_local_endpoint(&self) -> bool {
        reqwest::Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(|host| host.to_string()))
            .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("provider.ollama", self.timeout_secs, 10)
    }

    fn resolve_request_details(&self, model: &str) -> anyhow::Result<(String, bool)> {
        let requests_cloud = model.ends_with(":cloud");
        let normalized_model = model.strip_suffix(":cloud").unwrap_or(model).to_string();

        if requests_cloud && self.is_local_endpoint() {
            anyhow::bail!(
                "Model '{}' requested cloud routing, but Ollama endpoint is local. Configure api_url with a remote Ollama endpoint.",
                model
            );
        }

        if requests_cloud && self.api_key.is_none() {
            anyhow::bail!(
                "Model '{}' requested cloud routing, but no API key is configured. Set OLLAMA_API_KEY or config api_key.",
                model
            );
        }

        let should_auth = self.api_key.is_some() && !self.is_local_endpoint();

        Ok((normalized_model, should_auth))
    }

    fn parse_tool_arguments(arguments: &str) -> serde_json::Value {
        serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!({}))
    }

    fn normalize_response_text(content: String) -> Option<String> {
        let stripped = Self::strip_think_tags(&content);
        if stripped.trim().is_empty() {
            None
        } else {
            Some(stripped)
        }
    }

    fn strip_think_tags(s: &str) -> String {
        const OPEN_TAG: &str = "<think>";
        const CLOSE_TAG: &str = "</think>";
        let mut result = String::with_capacity(s.len());
        let mut rest = s;
        let mut depth: usize = 0;
        loop {
            let open_pos = rest.find(OPEN_TAG);
            let close_pos = rest.find(CLOSE_TAG);
            match (open_pos, close_pos) {
                (Some(open), Some(close)) if open < close => {
                    if depth == 0 {
                        result.push_str(&rest[..open]);
                    }
                    depth += 1;
                    rest = &rest[open + OPEN_TAG.len()..];
                }
                (_, Some(close)) => {
                    if depth > 0 {
                        depth -= 1;
                    } else {
                        result.push_str(&rest[..close]);
                    }
                    rest = &rest[close + CLOSE_TAG.len()..];
                }
                (Some(open), None) => {
                    if depth == 0 {
                        result.push_str(&rest[..open]);
                    }
                    depth += 1;
                    rest = &rest[open + OPEN_TAG.len()..];
                }
                (None, None) => {
                    if depth == 0 {
                        result.push_str(rest);
                    } else if result.trim().is_empty() {
                        return rest.trim().to_string();
                    }
                    break;
                }
            }
        }
        result.trim().to_string()
    }

    fn effective_content(content: &str, thinking: Option<&str>) -> Option<String> {

        let stripped = Self::strip_think_tags(content);
        if !stripped.trim().is_empty() {
            return Some(stripped);
        }

        if let Some(thinking) = thinking.map(str::trim).filter(|t| !t.is_empty()) {
            let stripped_thinking = Self::strip_think_tags(thinking);
            if !stripped_thinking.trim().is_empty() {
                tracing::debug!(
                    "Ollama: using thinking field as effective content ({} chars)",
                    stripped_thinking.len()
                );
                return Some(stripped_thinking);
            }
        }

        None
    }

    fn empty_content_error(model: &str, thinking: Option<&str>) -> anyhow::Error {
        // Return a real error (not fabricated first-person text): a fake reply
        // would suppress the reliable layer's retry/failover and pollute history.
        if let Some(thinking) = thinking.map(str::trim).filter(|value| !value.is_empty()) {
            let thinking_log_excerpt: String = thinking.chars().take(100).collect();
            tracing::warn!(
                "Ollama returned empty content with only thinking for model '{}': '{}'. Model may have stopped prematurely.",
                model,
                thinking_log_excerpt
            );
            return anyhow::anyhow!(
                "Ollama model '{model}' returned only reasoning with no answer content (stopped prematurely)"
            );
        }

        tracing::warn!(
            "Ollama returned empty or whitespace content with no tool calls for model '{}'",
            model
        );
        anyhow::anyhow!(
            "Ollama model '{model}' returned empty content with no tool calls"
        )
    }

    fn build_chat_request_with_think(
        &self,
        messages: Vec<Message>,
        model: &str,
        temperature: f64,
        tools: Option<&[serde_json::Value]>,
        think: Option<bool>,
    ) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            messages,
            stream: false,
            options: Options { temperature },
            think,
            tools: tools.map(|t| t.to_vec()),
        }
    }

    fn convert_user_message_content(&self, content: &str) -> (Option<String>, Option<Vec<String>>) {
        let (cleaned, image_refs) = multimodal::parse_image_markers(content);
        if image_refs.is_empty() {
            return (Some(content.to_string()), None);
        }

        let images: Vec<String> = image_refs
            .iter()
            .filter_map(|reference| multimodal::extract_ollama_image_payload(reference))
            .collect();

        if images.is_empty() {
            return (Some(content.to_string()), None);
        }

        let cleaned = cleaned.trim();
        let content = if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        };

        (content, Some(images))
    }

    fn convert_messages(&self, messages: &[ChatMessage]) -> Vec<Message> {
        let mut tool_name_by_id: HashMap<String, String> = HashMap::new();

        messages
            .iter()
            .map(|message| {
                if message.role == "assistant" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content) {
                        if let Some(tool_calls_value) = value.get("tool_calls") {
                            if let Ok(parsed_calls) =
                                serde_json::from_value::<Vec<ToolCall>>(tool_calls_value.clone())
                            {
                                let outgoing_calls: Vec<OutgoingToolCall> = parsed_calls
                                    .into_iter()
                                    .map(|call| {
                                        tool_name_by_id.insert(call.id.clone(), call.name.clone());
                                        OutgoingToolCall {
                                            kind: "function".to_string(),
                                            function: OutgoingFunction {
                                                name: call.name,
                                                arguments: Self::parse_tool_arguments(
                                                    &call.arguments,
                                                ),
                                            },
                                        }
                                    })
                                    .collect();
                                let content = value
                                    .get("content")
                                    .and_then(serde_json::Value::as_str)
                                    .map(ToString::to_string);
                                return Message {
                                    role: "assistant".to_string(),
                                    content,
                                    images: None,
                                    tool_calls: Some(outgoing_calls),
                                    tool_name: None,
                                };
                            }
                        }
                    }
                }

                if message.role == "tool" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content) {
                        let tool_name = value
                            .get("tool_name")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string)
                            .or_else(|| {
                                value
                                    .get("tool_call_id")
                                    .and_then(serde_json::Value::as_str)
                                    .and_then(|id| tool_name_by_id.get(id))
                                    .cloned()
                            });
                        let content = value
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string)
                            .or_else(|| {
                                (!message.content.trim().is_empty())
                                    .then_some(message.content.clone())
                            });

                        return Message {
                            role: "tool".to_string(),
                            content,
                            images: None,
                            tool_calls: None,
                            tool_name,
                        };
                    }
                }

                if message.role == "user" {
                    let (content, images) = self.convert_user_message_content(&message.content);
                    return Message {
                        role: "user".to_string(),
                        content,
                        images,
                        tool_calls: None,
                        tool_name: None,
                    };
                }

                Message {
                    role: message.role.clone(),
                    content: Some(message.content.clone()),
                    images: None,
                    tool_calls: None,
                    tool_name: None,
                }
            })
            .collect()
    }

    async fn send_request_inner(
        &self,
        messages: &[Message],
        model: &str,
        temperature: f64,
        should_auth: bool,
        tools: Option<&[serde_json::Value]>,
        think: Option<bool>,
    ) -> anyhow::Result<ApiChatResponse> {
        let request =
            self.build_chat_request_with_think(messages.to_vec(), model, temperature, tools, think);

        let url = format!("{}/api/chat", self.base_url);

        tracing::debug!(
            "Ollama request: url={} model={} message_count={} temperature={} think={:?} tool_count={}",
            url,
            model,
            request.messages.len(),
            temperature,
            request.think,
            request.tools.as_ref().map_or(0, |t| t.len()),
        );

        let mut request_builder = self.http_client().post(&url).json(&request);

        for (name, value) in &self.extra_headers {
            request_builder = request_builder.header(name, value);
        }

        if should_auth {
            if let Some(key) = self.api_key.as_ref() {
                request_builder = request_builder.bearer_auth(key);
            }
        }

        let response = request_builder.send().await?;
        let status = response.status();
        tracing::debug!("Ollama response status: {}", status);

        let body = response.bytes().await?;
        tracing::debug!("Ollama response body length: {} bytes", body.len());

        if !status.is_success() {
            let raw = String::from_utf8_lossy(&body);
            let sanitized = super::sanitize_api_error(&raw);
            tracing::error!(
                "Ollama error response: status={} body_excerpt={}",
                status,
                sanitized
            );
            anyhow::bail!(
                "Ollama API error ({}): {}. Is Ollama running? (brew install ollama && ollama serve)",
                status,
                sanitized
            );
        }

        let chat_response: ApiChatResponse = match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                let raw = String::from_utf8_lossy(&body);
                let sanitized = super::sanitize_api_error(&raw);
                tracing::error!(
                    "Ollama response deserialization failed: {e}. body_excerpt={}",
                    sanitized
                );
                anyhow::bail!("Failed to parse Ollama response: {e}");
            }
        };

        Ok(chat_response)
    }

    async fn send_request(
        &self,
        messages: Vec<Message>,
        model: &str,
        temperature: f64,
        should_auth: bool,
        tools: Option<&[serde_json::Value]>,
    ) -> anyhow::Result<ApiChatResponse> {
        let result = self
            .send_request_inner(
                &messages,
                model,
                temperature,
                should_auth,
                tools,
                self.reasoning_enabled,
            )
            .await;

        match result {
            Ok(resp) => Ok(resp),
            Err(first_err) if self.reasoning_enabled == Some(true) => {
                tracing::warn!(
                    model = model,
                    error = %first_err,
                    "Ollama request failed with think=true; retrying without reasoning \
                     (model may not support it)"
                );

                self.send_request_inner(&messages, model, temperature, should_auth, tools, None)
                    .await
                    .map_err(|retry_err| {

                        tracing::error!(
                            model = model,
                            original_error = %first_err,
                            retry_error = %retry_err,
                            "Ollama request also failed without think; returning original error"
                        );
                        first_err
                    })
            }
            Err(e) => Err(e),
        }
    }

    fn format_tool_calls_for_loop(&self, tool_calls: &[OllamaToolCall]) -> String {
        let formatted_calls: Vec<serde_json::Value> = tool_calls
            .iter()
            .map(|tc| {
                let (tool_name, tool_args) = self.extract_tool_name_and_args(tc);

                let args_str =
                    serde_json::to_string(&tool_args).unwrap_or_else(|_| "{}".to_string());

                serde_json::json!({
                    "id": tc.id,
                    "type": "function",
                    "function": {
                        "name": tool_name,
                        "arguments": args_str
                    }
                })
            })
            .collect();

        serde_json::json!({
            "content": "",
            "tool_calls": formatted_calls
        })
        .to_string()
    }

    fn extract_tool_name_and_args(&self, tc: &OllamaToolCall) -> (String, serde_json::Value) {
        let name = &tc.function.name;
        let args = &tc.function.arguments;

        if name == "tool_call"
            || name == "tool.call"
            || name.starts_with("tool_call>")
            || name.starts_with("tool_call<")
        {
            if let Some(nested_name) = args.get("name").and_then(|v| v.as_str()) {
                let nested_args = args
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                tracing::debug!(
                    "Unwrapped nested tool call: {} -> {} with args {:?}",
                    name,
                    nested_name,
                    nested_args
                );
                return (nested_name.to_string(), nested_args);
            }
        }

        if let Some(stripped) = name.strip_prefix("tool.") {
            return (stripped.to_string(), args.clone());
        }

        (name.clone(), args.clone())
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: false,
            vision: true,
            prompt_caching: false,
            responses_api: false,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let (normalized_model, should_auth) = self.resolve_request_details(model)?;

        let mut messages = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system".to_string(),
                content: Some(sys.to_string()),
                images: None,
                tool_calls: None,
                tool_name: None,
            });
        }

        let (user_content, user_images) = self.convert_user_message_content(message);
        messages.push(Message {
            role: "user".to_string(),
            content: user_content,
            images: user_images,
            tool_calls: None,
            tool_name: None,
        });

        let response = self
            .send_request(messages, &normalized_model, temperature, should_auth, None)
            .await?;

        if !response.message.tool_calls.is_empty() {
            tracing::debug!(
                "Ollama returned {} tool call(s), formatting for loop parser",
                response.message.tool_calls.len()
            );
            return Ok(self.format_tool_calls_for_loop(&response.message.tool_calls));
        }

        if let Some(content) = Self::effective_content(
            &response.message.content,
            response.message.thinking.as_deref(),
        ) {
            return Ok(content);
        }

        Err(Self::empty_content_error(
            &normalized_model,
            response.message.thinking.as_deref(),
        ))
    }

    async fn chat_with_history(
        &self,
        messages: &[crate::providers::ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let (normalized_model, should_auth) = self.resolve_request_details(model)?;

        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            model,
            0,
            None,
        );
        let api_messages = self.convert_messages(&sanitized_messages);

        let response = self
            .send_request(
                api_messages,
                &normalized_model,
                temperature,
                should_auth,
                None,
            )
            .await?;

        if !response.message.tool_calls.is_empty() {
            tracing::debug!(
                "Ollama returned {} tool call(s), formatting for loop parser",
                response.message.tool_calls.len()
            );
            return Ok(self.format_tool_calls_for_loop(&response.message.tool_calls));
        }

        if let Some(content) = Self::effective_content(
            &response.message.content,
            response.message.thinking.as_deref(),
        ) {
            return Ok(content);
        }

        Err(Self::empty_content_error(
            &normalized_model,
            response.message.thinking.as_deref(),
        ))
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let (normalized_model, should_auth) = self.resolve_request_details(model)?;

        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            model,
            0,
            None,
        );
        let api_messages = self.convert_messages(&sanitized_messages);

        let tools_opt = if tools.is_empty() { None } else { Some(tools) };

        let response = self
            .send_request(
                api_messages,
                &normalized_model,
                temperature,
                should_auth,
                tools_opt,
            )
            .await?;

        let usage = if response.prompt_eval_count.is_some() || response.eval_count.is_some() {
            Some(TokenUsage {
                input_tokens: response.prompt_eval_count,
                output_tokens: response.eval_count,
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
            })
        } else {
            None
        };

        if !response.message.tool_calls.is_empty() {
            let tool_calls: Vec<ToolCall> = response
                .message
                .tool_calls
                .iter()
                .map(|tc| {
                    let (name, args) = self.extract_tool_name_and_args(tc);
                    ToolCall {
                        id: tc
                            .id
                            .clone()
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                        name,
                        arguments: serde_json::to_string(&args)
                            .unwrap_or_else(|_| "{}".to_string()),
                    }
                })
                .collect();
            let text = Self::normalize_response_text(response.message.content);
            return Ok(ChatResponse {
                text,
                tool_calls,
                usage,
                reasoning_content: None,
            });
        }

        let effective = Self::effective_content(
            &response.message.content,
            response.message.thinking.as_deref(),
        );
        let Some(text) = effective else {
            return Err(Self::empty_content_error(
                &normalized_model,
                response.message.thinking.as_deref(),
            ));
        };
        Ok(ChatResponse::text_only(Some(text), usage))
    }

    fn supports_native_tools(&self) -> bool {

        false
    }

    async fn chat(
        &self,
        request: crate::providers::traits::ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {

        if let Some(specs) = request.tools {
            if !specs.is_empty() {
                let tools: Vec<serde_json::Value> = specs
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": s.name,
                                "description": s.description,
                                "parameters": s.parameters
                            }
                        })
                    })
                    .collect();
                return self
                    .chat_with_tools(request.messages, &tools, model, temperature)
                    .await;
            }
        }

        let text = self
            .chat_with_history(request.messages, model, temperature)
            .await?;
        Ok(ChatResponse::text_only(Some(text), None))
    }
}

