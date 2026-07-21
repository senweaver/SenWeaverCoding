// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, TokenUsage, ToolCall as ProviderToolCall, ToolsPayload,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_API_VERSION: &str = "2024-08-01-preview";

#[allow(dead_code)]
pub struct AzureOpenAiProvider {
    credential: Option<String>,
    resource_name: String,
    deployment_name: String,
    api_version: String,
    base_url: String,
    timeout_secs: u64,
    extra_headers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    messages: Vec<Message>,
    temperature: f64,
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
    messages: Vec<NativeMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<NativeToolSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "crate::providers::sanitize::skip_serializing_tool_calls")]
    tool_calls: Option<Vec<NativeToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

fn user_content_value(text: &str) -> serde_json::Value {
    let (cleaned_text, image_refs) = crate::multimodal::parse_image_markers(text);
    if image_refs.is_empty() {
        return serde_json::Value::String(text.to_string());
    }
    let mut parts: Vec<serde_json::Value> = Vec::with_capacity(image_refs.len() + 1);
    let trimmed = cleaned_text.trim();
    if !trimmed.is_empty() {
        parts.push(serde_json::json!({ "type": "text", "text": trimmed }));
    }
    for image_ref in image_refs {
        parts.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": image_ref },
        }));
    }
    serde_json::Value::Array(parts)
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
        .map_err(|e| anyhow::anyhow!("Invalid Azure OpenAI tool specification: {e}"))?;

    if spec.kind != "function" {
        anyhow::bail!(
            "Invalid Azure OpenAI tool specification: unsupported tool type '{}', expected 'function'",
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
}

#[derive(Debug, Deserialize)]
struct NativeChoice {
    message: NativeResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
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

impl AzureOpenAiProvider {
    pub fn new(
        credential: Option<&str>,
        resource_name: &str,
        deployment_name: &str,
        api_version: Option<&str>,
    ) -> Self {
        let version = api_version.unwrap_or(DEFAULT_API_VERSION);
        let base_url = format!(
            "https://{}.openai.azure.com/openai/deployments/{}",
            resource_name, deployment_name
        );
        Self {
            credential: credential.map(ToString::to_string),
            resource_name: resource_name.to_string(),
            deployment_name: deployment_name.to_string(),
            api_version: version.to_string(),
            base_url,
            timeout_secs: 120,
            extra_headers: std::collections::HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs.max(1);
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

    fn chat_completions_url(&self) -> String {
        format!(
            "{}/chat/completions?api-version={}",
            self.base_url, self.api_version
        )
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
                                    .map(|s| serde_json::Value::String(s.to_string()));
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
                            .map(|s| serde_json::Value::String(s.to_string()));
                        return NativeMessage {
                            role: "tool".to_string(),
                            content,
                            tool_call_id,
                            tool_calls: None,
                            reasoning_content: None,
                        };
                    }
                }

                let content = if m.role == "user" {
                    user_content_value(&m.content)
                } else {
                    serde_json::Value::String(m.content.clone())
                };
                NativeMessage {
                    role: m.role.clone(),
                    content: Some(content),
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                }
            })
            .collect()
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
                    id: tc.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
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
            thinking_signature: None,
            stop_reason: None,
        }
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("provider.azure_openai", self.timeout_secs, 10)
    }
}

#[async_trait]
impl Provider for AzureOpenAiProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
            prompt_caching: false,
            responses_api: false,
        }
    }

    fn convert_tools(&self, tools: &[ToolSpec]) -> ToolsPayload {
        ToolsPayload::OpenAI {
            tools: tools
                .iter()
                .map(|tool| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect(),
        }
    }

    fn supports_native_tools(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Azure OpenAI API key not set. Set AZURE_OPENAI_API_KEY or edit config.toml."
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
            messages,
            temperature,
        };

        let http_request = self
            .http_client()
            .post(self.chat_completions_url())
            .header("api-key", credential.as_str())
            .json(&request);
        let response = self.apply_extra_headers(http_request).send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("Azure OpenAI", response).await);
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.effective_content())
            .ok_or_else(|| anyhow::anyhow!("No response from Azure OpenAI"))
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Azure OpenAI API key not set. Set AZURE_OPENAI_API_KEY or edit config.toml."
            )
        })?;

        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            _model,
            0,
            None,
        );
        let tools = Self::convert_tools(request.tools);
        let native_request = NativeChatRequest {
            messages: Self::convert_messages(&sanitized_messages),
            temperature,
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
        };

        let http_request = self
            .http_client()
            .post(self.chat_completions_url())
            .header("api-key", credential.as_str())
            .json(&native_request);
        let response = self.apply_extra_headers(http_request).send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("Azure OpenAI", response).await);
        }

        let native_response: NativeChatResponse = response.json().await?;
        let usage = native_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
        });
        let choice = native_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from Azure OpenAI"))?;
        let stop_reason = choice
            .finish_reason
            .as_deref()
            .and_then(crate::providers::traits::StopReason::from_wire);
        let mut result = Self::parse_native_response(choice.message);
        result.usage = usage;
        result.stop_reason = stop_reason;
        Ok(result)
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Azure OpenAI API key not set. Set AZURE_OPENAI_API_KEY or edit config.toml."
            )
        })?;

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

        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            _model,
            0,
            None,
        );
        let native_request = NativeChatRequest {
            messages: Self::convert_messages(&sanitized_messages),
            temperature,
            tool_choice: native_tools.as_ref().map(|_| "auto".to_string()),
            tools: native_tools,
        };

        let http_request = self
            .http_client()
            .post(self.chat_completions_url())
            .header("api-key", credential.as_str())
            .json(&native_request);
        let response = self.apply_extra_headers(http_request).send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("Azure OpenAI", response).await);
        }

        let native_response: NativeChatResponse = response.json().await?;
        let usage = native_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
        });
        let choice = native_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from Azure OpenAI"))?;
        let stop_reason = choice
            .finish_reason
            .as_deref()
            .and_then(crate::providers::traits::StopReason::from_wire);
        let mut result = Self::parse_native_response(choice.message);
        result.usage = usage;
        result.stop_reason = stop_reason;
        Ok(result)
    }

    async fn warmup(&self) -> anyhow::Result<()> {

        Ok(())
    }
}
