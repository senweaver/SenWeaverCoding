// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, StreamChunk, StreamError, StreamEvent, StreamOptions,
    StreamResult, StructuredResponse, TokenUsage, ToolCall as ProviderToolCall,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use base64::Engine as _;
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct AnthropicProvider {
    credential: Option<String>,
    base_url: String,
    max_tokens: u32,
    timeout_secs: u64,
    extra_headers: std::collections::HashMap<String, String>,
}

const DEFAULT_ANTHROPIC_MAX_TOKENS: u32 = 4096;
const DEFAULT_ANTHROPIC_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Serialize)]
struct NativeChatRequest<'a> {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<SystemPrompt>,
    messages: Vec<NativeMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<NativeToolSpec<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    content: Vec<NativeContentOut>,
}

#[derive(Debug, Serialize)]
struct ImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum NativeContentOut {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        signature: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

#[derive(Debug, Serialize)]
struct NativeToolSpec<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    cache_type: String,
}

impl CacheControl {
    fn ephemeral() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum SystemPrompt {
    String(String),
    Blocks(Vec<SystemBlock>),
}

#[derive(Debug, Serialize)]
struct SystemBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Debug, Deserialize)]
struct NativeChatResponse {
    #[serde(default)]
    content: Vec<NativeContentIn>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NativeContentIn {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
    #[serde(default)]
    thinking: Option<String>,
}

impl AnthropicProvider {
    pub fn new(credential: Option<&str>) -> Self {
        Self::with_base_url(credential, None)
    }

    pub fn with_base_url(credential: Option<&str>, base_url: Option<&str>) -> Self {
        let base_url = base_url
            .map(|u| u.trim_end_matches('/'))
            .unwrap_or("https://api.anthropic.com")
            .to_string();
        Self {
            credential: credential
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(ToString::to_string),
            base_url,
            max_tokens: DEFAULT_ANTHROPIC_MAX_TOKENS,
            timeout_secs: DEFAULT_ANTHROPIC_TIMEOUT_SECS,
            extra_headers: std::collections::HashMap::new(),
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
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

    fn is_setup_token(token: &str) -> bool {
        token.starts_with("sk-ant-oat01-")
    }

    fn apply_auth(
        &self,
        request: reqwest::RequestBuilder,
        credential: &str,
    ) -> reqwest::RequestBuilder {
        if Self::is_setup_token(credential) {
            request
                .header("Authorization", format!("Bearer {credential}"))
                .header(
                    "anthropic-beta",
                    "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14",
                )
                .header("anthropic-dangerous-direct-browser-access", "true")
        } else {
            request.header("x-api-key", credential)
        }
    }

    fn apply_oauth_system_prompt(system: Option<SystemPrompt>) -> Option<SystemPrompt> {
        let prefix = SystemBlock {
            block_type: "text".to_string(),
            text: "You are Claude Code, Anthropic's official CLI for Claude.".to_string(),
            cache_control: Some(CacheControl::ephemeral()),
        };
        match system {
            Some(SystemPrompt::Blocks(mut blocks)) => {
                blocks.insert(0, prefix);
                Some(SystemPrompt::Blocks(blocks))
            }
            Some(SystemPrompt::String(s)) => Some(SystemPrompt::Blocks(vec![
                prefix,
                SystemBlock {
                    block_type: "text".to_string(),
                    text: s,
                    cache_control: Some(CacheControl::ephemeral()),
                },
            ])),
            None => Some(SystemPrompt::Blocks(vec![prefix])),
        }
    }

    fn should_cache_conversation(messages: &[ChatMessage]) -> bool {
        messages.iter().filter(|m| m.role != "system").count() > 1
    }

    fn should_cache_last_attachment(messages: &[ChatMessage]) -> bool {
        const ATTACHMENT_THRESHOLD: usize = 1024;
        messages
            .iter()
            .rev()
            .find(|m| m.role == "user" || m.role == "tool")
            .is_some_and(|m| m.content.len() >= ATTACHMENT_THRESHOLD)
    }

    fn apply_cache_to_last_message(messages: &mut [NativeMessage]) {
        if let Some(last_msg) = messages.last_mut() {
            if let Some(last_content) = last_msg.content.last_mut() {
                match last_content {
                    NativeContentOut::Text { cache_control, .. }
                    | NativeContentOut::ToolResult { cache_control, .. } => {
                        *cache_control = Some(CacheControl::ephemeral());
                    }
                    NativeContentOut::ToolUse { .. }
                    | NativeContentOut::Image { .. }
                    | NativeContentOut::Thinking { .. } => {}
                }
            }
        }
    }

    fn convert_tools<'a>(tools: Option<&'a [ToolSpec]>) -> Option<Vec<NativeToolSpec<'a>>> {
        let items = tools?;
        if items.is_empty() {
            return None;
        }
        let mut seen: std::collections::HashSet<&str> =
            std::collections::HashSet::with_capacity(items.len());
        let mut native_tools: Vec<NativeToolSpec<'a>> = items
            .iter()
            .filter(|tool| seen.insert(tool.name.as_str()))
            .map(|tool| NativeToolSpec {
                name: &tool.name,
                description: &tool.description,
                input_schema: &tool.parameters,
                cache_control: None,
            })
            .collect();
        if native_tools.is_empty() {
            return None;
        }

        if let Some(last_tool) = native_tools.last_mut() {
            last_tool.cache_control = Some(CacheControl::ephemeral());
        }

        Some(native_tools)
    }

    fn parse_assistant_tool_call_message(content: &str) -> Option<Vec<NativeContentOut>> {
        let trimmed = content.trim_start();
        if trimmed.starts_with('[') {
            return Self::parse_assistant_native_block_array(trimmed);
        }
        let value = serde_json::from_str::<serde_json::Value>(content).ok()?;
        let tool_calls = value
            .get("tool_calls")
            .and_then(|v| serde_json::from_value::<Vec<ProviderToolCall>>(v.clone()).ok())?;

        let mut blocks = Vec::new();
        if let Some(block) = Self::thinking_block_from_envelope(&value) {
            blocks.push(block);
        }
        if let Some(text) = value
            .get("content")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            blocks.push(NativeContentOut::Text {
                text: text.to_string(),
                cache_control: None,
            });
        }
        for call in tool_calls {
            let input = serde_json::from_str::<serde_json::Value>(&call.arguments)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
            let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                Some(call.id),
                crate::providers::sanitize::ProviderKind::Anthropic,
            );
            blocks.push(NativeContentOut::ToolUse {
                id,
                name: call.name,
                input,
                cache_control: None,
            });
        }
        Some(blocks)
    }

    fn parse_assistant_native_block_array(content: &str) -> Option<Vec<NativeContentOut>> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(content).ok()?;
        if arr.is_empty() {
            return None;
        }
        let mut blocks: Vec<NativeContentOut> = Vec::new();
        let mut has_tool_use = false;
        for item in arr {
            let kind = match item.get("type").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => continue,
            };
            match kind {
                "thinking" => {
                    let thinking = item
                        .get("thinking")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let signature = item
                        .get("signature")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_default();
                    if let Some(thinking) = thinking.filter(|s| !s.is_empty()) {
                        blocks.push(NativeContentOut::Thinking {
                            thinking,
                            signature,
                        });
                    }
                }
                "text" => {
                    let text = item
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .filter(|s| !s.is_empty());
                    if let Some(text) = text {
                        blocks.push(NativeContentOut::Text {
                            text,
                            cache_control: None,
                        });
                    }
                }
                "tool_use" => {
                    let raw_id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .or_else(|| item.get("tool_use_id").and_then(|v| v.as_str()))
                        .or_else(|| item.get("tool_call_id").and_then(|v| v.as_str()))
                        .map(str::to_string);
                    let name = match item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                    {
                        Some(n) if !n.is_empty() => n,
                        _ => continue,
                    };
                    let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                        raw_id,
                        crate::providers::sanitize::ProviderKind::Anthropic,
                    );
                    let input = item
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
                    has_tool_use = true;
                    blocks.push(NativeContentOut::ToolUse {
                        id,
                        name,
                        input,
                        cache_control: None,
                    });
                }
                _ => {}
            }
        }
        if !has_tool_use {
            return None;
        }
        Some(blocks)
    }

    fn extract_thinking_block(msg: &ChatMessage) -> Option<NativeContentOut> {
        let thinking = msg
            .metadata
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())?;
        let signature = msg
            .metadata
            .get("thinking_signature")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        Some(NativeContentOut::Thinking {
            thinking: thinking.to_string(),
            signature: signature.to_string(),
        })
    }

    fn thinking_block_from_envelope(value: &serde_json::Value) -> Option<NativeContentOut> {
        let thinking = value
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())?;
        let signature = value
            .get("thinking_signature")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        Some(NativeContentOut::Thinking {
            thinking: thinking.to_string(),
            signature: signature.to_string(),
        })
    }

    fn parse_tool_result_message(content: &str) -> Option<NativeMessage> {
        let trimmed = content.trim_start();
        if trimmed.starts_with('[') {
            return Self::parse_tool_result_block_array(trimmed);
        }
        let value = serde_json::from_str::<serde_json::Value>(content).ok()?;
        let tool_use_id = value
            .get("tool_use_id")
            .and_then(serde_json::Value::as_str)
            .or_else(|| value.get("tool_call_id").and_then(serde_json::Value::as_str))?
            .to_string();
        let result = value
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        Some(NativeMessage {
            role: "user".to_string(),
            content: vec![NativeContentOut::ToolResult {
                tool_use_id,
                content: result,
                cache_control: None,
            }],
        })
    }

    fn parse_tool_result_block_array(content: &str) -> Option<NativeMessage> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(content).ok()?;
        let mut blocks: Vec<NativeContentOut> = Vec::new();
        for v in arr {
            if v.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let tool_use_id = v
                .get("tool_use_id")
                .and_then(|t| t.as_str())
                .or_else(|| v.get("tool_call_id").and_then(|t| t.as_str()))
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())?;
            let body = match v.get("content") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            blocks.push(NativeContentOut::ToolResult {
                tool_use_id,
                content: body,
                cache_control: None,
            });
        }
        if blocks.is_empty() {
            return None;
        }
        Some(NativeMessage {
            role: "user".to_string(),
            content: blocks,
        })
    }

    fn convert_messages(messages: &[ChatMessage]) -> (Option<SystemPrompt>, Vec<NativeMessage>) {
        let mut system_parts: Vec<String> = Vec::new();
        let mut native_messages = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    if !msg.content.trim().is_empty() {
                        system_parts.push(msg.content.clone());
                    }
                }
                "assistant" => {
                    if let Some(blocks) = Self::parse_assistant_tool_call_message(&msg.content) {
                        native_messages.push(NativeMessage {
                            role: "assistant".to_string(),
                            content: blocks,
                        });
                    } else if !msg.content.trim().is_empty() {
                        let mut blocks: Vec<NativeContentOut> = Vec::new();
                        if let Some(block) = Self::extract_thinking_block(msg) {
                            blocks.push(block);
                        }
                        blocks.push(NativeContentOut::Text {
                            text: msg.content.clone(),
                            cache_control: None,
                        });
                        native_messages.push(NativeMessage {
                            role: "assistant".to_string(),
                            content: blocks,
                        });
                    } else if let Some(block) = Self::extract_thinking_block(msg) {
                        native_messages.push(NativeMessage {
                            role: "assistant".to_string(),
                            content: vec![block],
                        });
                    }
                }
                "tool" => {
                    let tool_msg = if let Some(tr) = Self::parse_tool_result_message(&msg.content) {
                        tr
                    } else if !msg.content.trim().is_empty() {
                        NativeMessage {
                            role: "user".to_string(),
                            content: vec![NativeContentOut::Text {
                                text: msg.content.clone(),
                                cache_control: None,
                            }],
                        }
                    } else {
                        continue;
                    };

                    let same_role = native_messages
                        .last()
                        .is_some_and(|m| m.role == tool_msg.role);
                    if let (true, Some(last)) = (same_role, native_messages.last_mut()) {
                        last.content.extend(tool_msg.content);
                    } else {
                        native_messages.push(tool_msg);
                    }
                }
                _ => {

                    let (text, image_refs) = crate::multimodal::parse_image_markers(&msg.content);
                    let mut content_blocks: Vec<NativeContentOut> = Vec::new();

                    for img_ref in &image_refs {
                        let (media_type, data) = if img_ref.starts_with("data:") {

                            if let Some(comma) = img_ref.find(',') {
                                let header = &img_ref[5..comma];
                                let mime =
                                    header.split(';').next().unwrap_or("image/jpeg").to_string();
                                let b64 = img_ref[comma + 1..].trim().to_string();
                                (mime, b64)
                            } else {
                                continue;
                            }
                        } else if std::path::Path::new(img_ref.trim()).exists() {

                            match std::fs::read(img_ref.trim()) {
                                Ok(bytes) => {
                                    let b64 =
                                        base64::engine::general_purpose::STANDARD.encode(&bytes);
                                    let ext = std::path::Path::new(img_ref.trim())
                                        .extension()
                                        .and_then(|e| e.to_str())
                                        .unwrap_or("jpg");
                                    let mime = match ext {
                                        "png" => "image/png",
                                        "gif" => "image/gif",
                                        "webp" => "image/webp",
                                        _ => "image/jpeg",
                                    }
                                    .to_string();
                                    (mime, b64)
                                }
                                Err(_) => continue,
                            }
                        } else {
                            continue;
                        };

                        content_blocks.push(NativeContentOut::Image {
                            source: ImageSource {
                                source_type: "base64".to_string(),
                                media_type,
                                data,
                            },
                        });
                    }

                    if text.is_empty() && !image_refs.is_empty() {
                        content_blocks.push(NativeContentOut::Text {
                            text: "[image]".to_string(),
                            cache_control: None,
                        });
                    } else if !text.trim().is_empty() {
                        content_blocks.push(NativeContentOut::Text {
                            text,
                            cache_control: None,
                        });
                    }

                    // Never emit a user message with an empty content array:
                    // Anthropic rejects it with a 400 (`content: field required`).
                    if !content_blocks.is_empty() {
                        let same_role = native_messages.last().is_some_and(|m| m.role == "user");
                        if let (true, Some(last)) = (same_role, native_messages.last_mut()) {
                            last.content.extend(content_blocks);
                        } else {
                            native_messages.push(NativeMessage {
                                role: "user".to_string(),
                                content: content_blocks,
                            });
                        }
                    }
                }
            }
        }

        let system_prompt = if system_parts.is_empty() {
            None
        } else {
            Some(SystemPrompt::Blocks(vec![SystemBlock {
                block_type: "text".to_string(),
                text: system_parts.join("\n\n"),
                cache_control: Some(CacheControl::ephemeral()),
            }]))
        };

        (system_prompt, native_messages)
    }

    fn parse_native_response(response: NativeChatResponse) -> ProviderChatResponse {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut thinking_parts: Vec<String> = Vec::new();

        let usage = response.usage.map(|u| {
            let cached = u.cache_read_input_tokens;
            let created = u.cache_creation_input_tokens;
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, created);
            TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: created,
            }
        });

        for block in response.content {
            match block.kind.as_str() {
                "text" => {
                    if let Some(text) = block.text.map(|t| t.trim().to_string()) {
                        if !text.is_empty() {
                            text_parts.push(text);
                        }
                    }
                }
                "thinking" => {
                    if let Some(t) = block.thinking.filter(|s| !s.is_empty()) {
                        thinking_parts.push(t);
                    }
                }
                "tool_use" => {
                    let name = block.name.unwrap_or_default();
                    if name.is_empty() {
                        continue;
                    }
                    let arguments = block
                        .input
                        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
                    tool_calls.push(ProviderToolCall {
                        id: crate::providers::sanitize::normalize_tool_call_id_for_provider(
                            block.id,
                            crate::providers::sanitize::ProviderKind::Anthropic,
                        ),
                        name,
                        arguments: arguments.to_string(),
                    });
                }
                _ => {}
            }
        }

        let reasoning_content = if thinking_parts.is_empty() {
            None
        } else {
            Some(thinking_parts.join("\n"))
        };

        ProviderChatResponse {
            text: if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            },
            tool_calls,
            usage,
            reasoning_content,
        }
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts_and_headers(
                "provider.anthropic",
                self.timeout_secs,
                10,
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
                "provider.anthropic.stream",
                read_timeout_secs,
                10,
                &headers,
            )
    }

    fn build_streaming_request(
        request: &NativeChatRequest<'_>,
    ) -> serde_json::Result<serde_json::Value> {
        let mut body = serde_json::to_value(request)?;
        body["stream"] = serde_json::Value::Bool(true);
        Ok(body)
    }

    async fn parse_anthropic_sse(
        response: reqwest::Response,
        tx: &tokio::sync::mpsc::Sender<StreamResult<StreamEvent>>,
    ) {
        use tokio::io::AsyncBufReadExt;
        use tokio_util::io::StreamReader;

        let byte_stream = response
            .bytes_stream()
            .map(|result| result.map_err(std::io::Error::other));
        let reader = StreamReader::new(byte_stream);
        let mut lines = reader.lines();

        let mut tool_id: Option<String> = None;
        let mut tool_name: Option<String> = None;
        let mut tool_input_json = String::new();
        let mut made_progress = false;
        let mut usage_acc = AnthropicStreamUsage::default();

        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Provider(format!(
                            "anthropic stream read failed before completion: {e}"
                        ))))
                        .await;
                    return;
                }
            };
            let line = line.trim().to_string();
            let Some(json_str) = line
                .strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
            else {
                continue;
            };

            let event: serde_json::Value = match serde_json::from_str(json_str) {
                Ok(v) => v,
                Err(_) => {
                    tracing::debug!("Skipping malformed SSE event: {}", json_str);
                    continue;
                }
            };

            let event_type = event
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or_default();

            match event_type {
                "message_start" => {
                    if let Some(usage) = event.get("message").and_then(|m| m.get("usage")) {
                        usage_acc.merge(usage);
                    }
                }
                "message_delta" => {
                    if let Some(usage) = event.get("usage") {
                        usage_acc.merge(usage);
                    }
                }
                "content_block_start" => {
                    if let Some(block) = event.get("content_block") {
                        let block_type = block
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default();
                        if block_type == "tool_use" {
                            if tool_id.is_some() {
                                let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                                    tool_id.take(),
                                    crate::providers::sanitize::ProviderKind::Anthropic,
                                );
                                let name = tool_name.take().unwrap_or_default();
                                let input = std::mem::take(&mut tool_input_json);
                                let safe_input = sanitize_tool_call_arguments(input);
                                made_progress = true;
                                let _ = tx
                                    .send(Ok(StreamEvent::ToolCall(ProviderToolCall {
                                        id,
                                        name,
                                        arguments: safe_input,
                                    })))
                                    .await;
                            }
                            tool_id = block
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(ToString::to_string);
                            tool_name = block
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(ToString::to_string);
                            tool_input_json.clear();
                        }
                    }
                }
                "content_block_delta" => {
                    if let Some(delta) = event.get("delta") {
                        let delta_type = delta
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default();
                        match delta_type {
                            "text_delta" => {
                                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        made_progress = true;
                                        if tx
                                            .send(Ok(StreamEvent::TextDelta(StreamChunk::delta(
                                                text.to_string(),
                                            ))))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                            }
                            "thinking_delta" => {
                                if let Some(text) =
                                    delta.get("thinking").and_then(|t| t.as_str())
                                {
                                    if !text.is_empty() {
                                        made_progress = true;
                                        if tx
                                            .send(Ok(StreamEvent::TextDelta(
                                                StreamChunk::reasoning(text.to_string()),
                                            )))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                            }
                            "input_json_delta" => {
                                if let Some(json) =
                                    delta.get("partial_json").and_then(|j| j.as_str())
                                {
                                    tool_input_json.push_str(json);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "content_block_stop" => {
                    if tool_id.is_some() {
                        let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                            tool_id.take(),
                            crate::providers::sanitize::ProviderKind::Anthropic,
                        );
                        let name = tool_name.take().unwrap_or_default();
                        let input = std::mem::take(&mut tool_input_json);
                        let safe_input = sanitize_tool_call_arguments(input);
                        made_progress = true;
                        let _ = tx
                            .send(Ok(StreamEvent::ToolCall(ProviderToolCall {
                                id,
                                name,
                                arguments: safe_input,
                            })))
                            .await;
                    }
                }
                "message_stop" => {
                    flush_pending_tool_call(&mut tool_id, &mut tool_name, &mut tool_input_json, tx)
                        .await;
                    if let Some(usage) = usage_acc.into_token_usage() {
                        let _ = tx.send(Ok(StreamEvent::Usage(usage))).await;
                    }
                    let _ = tx.send(Ok(StreamEvent::Final)).await;
                    return;
                }
                "error" => {
                    // Deliberately do NOT flush any half-streamed tool_use here: the
                    // stream errored mid-response, so its arguments are incomplete
                    // and must not be executed. The pending state is simply dropped.
                    let msg = event
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown streaming error");
                    let sanitized = super::sanitize_api_error(msg);
                    let _ = tx.send(Err(StreamError::Provider(sanitized))).await;
                    return;
                }
                _ => {}
            }
        }

        if tool_id.is_some() || tool_name.is_some() || !tool_input_json.is_empty() {
            let _ = tx
                .send(Err(StreamError::Provider(
                    "anthropic stream ended before message_stop while a tool_use block was still streaming; connection closed mid-response".to_string(),
                )))
                .await;
            return;
        }
        if !made_progress {
            let _ = tx
                .send(Err(StreamError::Provider(
                    "anthropic stream reached EOF without message_stop and without any text/tool output; connection closed mid-response (truncated)".to_string(),
                )))
                .await;
            return;
        }
        if let Some(usage) = usage_acc.into_token_usage() {
            let _ = tx.send(Ok(StreamEvent::Usage(usage))).await;
        }
        let _ = tx.send(Ok(StreamEvent::Final)).await;
    }
}

#[derive(Debug, Default)]
struct AnthropicStreamUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
}

impl AnthropicStreamUsage {
    fn merge(&mut self, usage: &serde_json::Value) {
        if let Some(v) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
            self.input_tokens = Some(v);
        }
        if let Some(v) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
            self.output_tokens = Some(v);
        }
        if let Some(v) = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
        {
            self.cache_read_input_tokens = Some(v);
        }
        if let Some(v) = usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
        {
            self.cache_creation_input_tokens = Some(v);
        }
    }

    fn into_token_usage(self) -> Option<TokenUsage> {
        if self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cache_read_input_tokens.is_none()
            && self.cache_creation_input_tokens.is_none()
        {
            return None;
        }
        Some(TokenUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cached_input_tokens: self.cache_read_input_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens,
        })
    }
}

fn sanitize_tool_call_arguments(raw: String) -> String {
    if raw.trim().is_empty() {
        return "{}".to_string();
    }
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(_) => raw,
        Err(err) => {
            if let Some(repaired) =
                crate::providers::sanitize::repair_partial_tool_input_json(&raw)
            {
                tracing::warn!(
                    target: "providers.anthropic.stream",
                    error = %err,
                    raw_len = raw.len(),
                    repaired_len = repaired.len(),
                    "anthropic SSE delivered truncated tool_input JSON; recovered partial arguments via structural repair"
                );
                return repaired;
            }
            tracing::warn!(
                target: "providers.anthropic.stream",
                error = %err,
                raw_len = raw.len(),
                "anthropic SSE produced unrecoverable tool_input JSON; substituting empty object"
            );
            "{}".to_string()
        }
    }
}

async fn flush_pending_tool_call(
    tool_id: &mut Option<String>,
    tool_name: &mut Option<String>,
    tool_input_json: &mut String,
    tx: &tokio::sync::mpsc::Sender<StreamResult<StreamEvent>>,
) {
    if tool_id.is_none() && tool_name.is_none() && tool_input_json.is_empty() {
        return;
    }
    let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
        tool_id.take(),
        crate::providers::sanitize::ProviderKind::Anthropic,
    );
    let name = tool_name.take().unwrap_or_default();
    let input = std::mem::take(tool_input_json);
    if name.is_empty() {
        tracing::warn!(
            target: "provider.stream",
            "discarding anthropic tool_use flush with empty name (malformed/incomplete block)"
        );
        return;
    }
    let safe_input = sanitize_tool_call_arguments(input);
    let _ = tx
        .send(Ok(StreamEvent::ToolCall(ProviderToolCall {
            id,
            name,
            arguments: safe_input,
        })))
        .await;
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Anthropic credentials not set. Set ANTHROPIC_API_KEY or ANTHROPIC_OAUTH_TOKEN (setup-token)."
            )
        })?;

        let system = system_prompt.map(|s| SystemPrompt::String(s.to_string()));
        let system = if Self::is_setup_token(credential) {
            Self::apply_oauth_system_prompt(system)
        } else {
            system
        };

        let request = NativeChatRequest {
            model: model.to_string(),
            max_tokens: self.max_tokens,
            system,
            messages: vec![NativeMessage {
                role: "user".to_string(),
                content: vec![NativeContentOut::Text {
                    text: message.to_string(),
                    cache_control: None,
                }],
            }],
            temperature,
            tools: None,
            tool_choice: None,
            stream: None,
        };

        let mut request = self
            .http_client()
            .post(format!("{}/v1/messages", self.base_url))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request);

        request = self.apply_auth(request, credential);

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("Anthropic", response).await);
        }

        let chat_response: NativeChatResponse = response.json().await?;
        let parsed = Self::parse_native_response(chat_response);
        parsed
            .text
            .ok_or_else(|| anyhow::anyhow!("No response from Anthropic"))
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Anthropic credentials not set. Set ANTHROPIC_API_KEY or ANTHROPIC_OAUTH_TOKEN (setup-token)."
            )
        })?;

        let messages_owned = std::sync::Arc::new(
            crate::providers::sanitize::sanitize_messages_before_send_for_provider(
                request.messages.to_vec(),
                model,
                self.max_tokens as usize,
                None,
                crate::providers::sanitize::ProviderKind::Anthropic,
            ),
        );
        let (system_prompt, mut messages) = {
            let messages_for_blocking = std::sync::Arc::clone(&messages_owned);
            tokio::task::spawn_blocking(move || Self::convert_messages(&messages_for_blocking))
                .await
                .map_err(|e| anyhow::anyhow!("anthropic convert_messages join error: {e}"))?
        };

        if Self::should_cache_conversation(&messages_owned)
            || Self::should_cache_last_attachment(&messages_owned)
        {
            Self::apply_cache_to_last_message(&mut messages);
        }

        let tool_choice_override = crate::agent::loop_::TOOL_CHOICE_OVERRIDE
            .try_with(Clone::clone)
            .ok()
            .flatten();
        let native_tools = Self::convert_tools(request.tools);
        let tool_choice = if native_tools.is_some() {
            tool_choice_override.map(|tc| serde_json::json!({ "type": tc }))
        } else {
            None
        };

        let system_prompt = if Self::is_setup_token(credential) {
            Self::apply_oauth_system_prompt(system_prompt)
        } else {
            system_prompt
        };
        let native_request = NativeChatRequest {
            model: model.to_string(),
            max_tokens: self.max_tokens,
            system: system_prompt,
            messages,
            temperature,
            tools: native_tools,
            tool_choice,
            stream: None,
        };

        let req = self
            .http_client()
            .post(format!("{}/v1/messages", self.base_url))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&native_request);

        let response = self.apply_auth(req, credential).send().await?;
        if !response.status().is_success() {
            return Err(super::api_error("Anthropic", response).await);
        }

        let native_response: NativeChatResponse = response.json().await?;
        Ok(Self::parse_native_response(native_response))
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
            prompt_caching: true,
            responses_api: false,
        }
    }

    fn message_format_kind(&self) -> crate::providers::sanitize::ProviderKind {
        crate::providers::sanitize::ProviderKind::Anthropic
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

        let tool_specs: Vec<ToolSpec> = tools
            .iter()
            .filter_map(|t| {
                let func = t.get("function").or_else(|| {
                    tracing::warn!("Skipping malformed tool definition (missing 'function' key)");
                    None
                })?;
                let name = func.get("name").and_then(|n| n.as_str()).or_else(|| {
                    tracing::warn!("Skipping tool with missing or non-string 'name'");
                    None
                })?;
                Some(ToolSpec {
                    name: name.to_string(),
                    description: func
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string(),
                    parameters: func
                        .get("parameters")
                        .cloned()
                        .unwrap_or(serde_json::json!({"type": "object"})),
                })
            })
            .collect();

        let request = ProviderChatRequest {
            messages,
            tools: if tool_specs.is_empty() {
                None
            } else {
                Some(&tool_specs)
            },
        };
        self.chat(request, model, temperature).await
    }

    async fn chat_structured(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<StructuredResponse> {
        const TOOL_NAME: &str = "structured_output";

        let virtual_tool = ToolSpec {
            name: TOOL_NAME.to_string(),
            description: "Emit the structured payload conforming to the requested JSON schema."
                .to_string(),
            parameters: schema.clone(),
        };
        let tools = vec![virtual_tool];
        let request = ProviderChatRequest {
            messages,
            tools: Some(&tools),
        };

        let response = crate::agent::loop_::TOOL_CHOICE_OVERRIDE
            .scope(
                Some("any".to_string()),
                self.chat(request, model, temperature),
            )
            .await?;

        let call = response.tool_calls.into_iter().find(|c| c.name == TOOL_NAME);
        let raw = call
            .as_ref()
            .map(|c| c.arguments.clone())
            .unwrap_or_else(|| response.text.clone().unwrap_or_default());
        let value = serde_json::from_str::<serde_json::Value>(&raw).map_err(|err| {
            anyhow::anyhow!("Anthropic structured chat returned invalid JSON: {err} ({raw})")
        })?;
        Ok(StructuredResponse {
            data: value,
            raw_text: raw,
            usage: response.usage,
        })
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        if let Some(credential) = self.credential.as_ref() {
            let mut request = self
                .http_client()
                .post(format!("{}/v1/messages", self.base_url))
                .header("anthropic-version", "2023-06-01");
            request = self.apply_auth(request, credential);

            let _ = request.send().await?;
        }
        Ok(())
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
        if !options.enabled {
            return stream::once(async { Ok(StreamEvent::Final) }).boxed();
        }

        let credential = match self.credential.as_ref() {
            Some(c) => c.clone(),
            None => {
                return stream::once(async {
                    Err(StreamError::Provider(
                        "Anthropic credentials not set".to_string(),
                    ))
                })
                .boxed();
            }
        };

        let sanitized_messages =
            crate::providers::sanitize::sanitize_messages_before_send_for_provider(
                request.messages.to_vec(),
                model,
                self.max_tokens as usize,
                None,
                crate::providers::sanitize::ProviderKind::Anthropic,
            );

        let tool_choice_override = crate::agent::loop_::TOOL_CHOICE_OVERRIDE
            .try_with(Clone::clone)
            .ok()
            .flatten();
        let request_tools: Option<Vec<ToolSpec>> = request.tools.map(<[ToolSpec]>::to_vec);

        let model_owned = model.to_string();
        let max_tokens = self.max_tokens;
        let client = self.stream_http_client();
        let url = format!("{}/v1/messages", self.base_url);
        let is_oauth = Self::is_setup_token(&credential);

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(64);

        let _bg = crate::runtime::spawn_supervised("providers.anthropic.stream", async move {
            let (system_prompt, messages) = match tokio::task::spawn_blocking(move || {
                let (system_prompt, messages) = Self::convert_messages(&sanitized_messages);
                let cache = Self::should_cache_conversation(&sanitized_messages)
                    || Self::should_cache_last_attachment(&sanitized_messages);
                (system_prompt, messages, cache)
            })
            .await
            {
                Ok((system_prompt, messages, cache)) => {
                    let mut messages = messages;
                    if cache {
                        Self::apply_cache_to_last_message(&mut messages);
                    }
                    (system_prompt, messages)
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Provider(format!(
                            "anthropic convert_messages join error: {e}"
                        ))))
                        .await;
                    return;
                }
            };

            let native_tools = Self::convert_tools(request_tools.as_deref());
            let tool_choice = if native_tools.is_some() {
                tool_choice_override.map(|tc| serde_json::json!({ "type": tc }))
            } else {
                None
            };

            let system_prompt = if is_oauth {
                Self::apply_oauth_system_prompt(system_prompt)
            } else {
                system_prompt
            };

            let native_request = NativeChatRequest {
                model: model_owned,
                max_tokens,
                system: system_prompt,
                messages,
                temperature,
                tools: native_tools,
                tool_choice,
                stream: Some(true),
            };

            let body = match Self::build_streaming_request(&native_request) {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Provider(format!(
                            "anthropic request serialization error: {e}"
                        ))))
                        .await;
                    return;
                }
            };

            let mut req = client
                .post(&url)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body);

            if is_oauth {
                req = req
                    .header("Authorization", format!("Bearer {credential}"))
                    .header(
                        "anthropic-beta",
                        "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14",
                    )
                    .header("anthropic-dangerous-direct-browser-access", "true");
            } else {
                req = req.header("x-api-key", &credential);
            }

            let response = match req.send().await {
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
                let _ = tx
                    .send(Err(StreamError::Provider(format!("{status}: {sanitized}"))))
                    .await;
                return;
            }

            Self::parse_anthropic_sse(response, &tx).await;
        });

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })
        .boxed()
    }
}
