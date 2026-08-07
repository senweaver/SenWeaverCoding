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
    explicit_max_tokens: bool,
    reasoning_enabled: bool,
    reasoning_effort: Option<String>,
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
    thinking: Option<ThinkingParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<NativeToolSpec<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct ThinkingParam {
    #[serde(rename = "type")]
    thinking_type: String,
    budget_tokens: u32,
}

impl ThinkingParam {
    fn for_effort(effort: &str) -> Self {
        let budget_tokens = match effort.trim().to_ascii_lowercase().as_str() {
            "low" | "minimal" => 4_096,
            "high" | "max" | "xhigh" => 24_576,
            _ => 10_240,
        };
        Self {
            thinking_type: "enabled".to_string(),
            budget_tokens,
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<String>,
}

impl CacheControl {
    fn ephemeral() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: None,
        }
    }

    fn ephemeral_1h() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: Some("1h".to_string()),
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
    #[serde(default)]
    stop_reason: Option<String>,
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
    #[serde(default)]
    signature: Option<String>,
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
            explicit_max_tokens: false,
            reasoning_enabled: false,
            reasoning_effort: None,
            timeout_secs: DEFAULT_ANTHROPIC_TIMEOUT_SECS,
            extra_headers: std::collections::HashMap::new(),
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self.explicit_max_tokens = true;
        self
    }

    pub fn with_reasoning(mut self, enabled: Option<bool>, effort: Option<String>) -> Self {
        self.reasoning_enabled = enabled.unwrap_or(false);
        self.reasoning_effort = effort;
        self
    }

    fn model_output_ceiling(model: &str) -> u32 {
        let id = model.rsplit('/').next().unwrap_or(model).to_ascii_lowercase();
        if id.contains("claude-3-7") || id.contains("claude-3.7") {
            64_000
        } else if id.contains("claude-3-5") || id.contains("claude-3.5") {
            8_192
        } else if id.contains("claude-3") {
            4_096
        } else if id.contains("opus-4-1")
            || id.contains("opus-4.1")
            || id.contains("opus-4-0")
            || id.contains("opus-4.0")
            || id == "claude-opus-4"
            || id.contains("claude-opus-4-2")
        {
            32_000
        } else if id.contains("sonnet-4")
            || id.contains("opus-4")
            || id.contains("haiku-4")
        {
            64_000
        } else if id.contains("claude") {
            64_000
        } else {
            crate::constants::api_limits::max_output_for_model(&id)
        }
    }

    fn effective_max_tokens(&self, model: &str) -> u32 {
        let ceiling = Self::model_output_ceiling(model);
        if self.explicit_max_tokens {
            self.max_tokens.min(ceiling)
        } else {
            ceiling
        }
    }

    fn thinking_param(&self) -> Option<ThinkingParam> {
        if !self.reasoning_enabled {
            return None;
        }
        Some(ThinkingParam::for_effort(
            self.reasoning_effort.as_deref().unwrap_or("medium"),
        ))
    }

    fn request_tuning(&self, model: &str, temperature: f64) -> (u32, f64, Option<ThinkingParam>) {
        let ceiling = Self::model_output_ceiling(model);
        let thinking = if ceiling > 16_384 {
            self.thinking_param()
        } else {
            None
        };
        let mut max_tokens = self.effective_max_tokens(model);
        let temperature = if let Some(t) = thinking.as_ref() {
            max_tokens = max_tokens
                .max(t.budget_tokens.saturating_add(4_096))
                .min(ceiling);
            1.0
        } else {
            temperature
        };
        (max_tokens, temperature, thinking)
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
                    "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,extended-cache-ttl-2025-04-11",
                )
                .header("anthropic-dangerous-direct-browser-access", "true")
        } else {
            request
                .header("x-api-key", credential)
                .header("anthropic-beta", "extended-cache-ttl-2025-04-11")
        }
    }

    fn apply_oauth_system_prompt(system: Option<SystemPrompt>) -> Option<SystemPrompt> {
        let prefix_uncached = SystemBlock {
            block_type: "text".to_string(),
            text: "You are Claude Code, Anthropic's official CLI for Claude.".to_string(),
            cache_control: None,
        };
        match system {
            Some(SystemPrompt::Blocks(mut blocks)) => {
                blocks.insert(0, prefix_uncached);
                Some(SystemPrompt::Blocks(blocks))
            }
            Some(SystemPrompt::String(s)) => Some(SystemPrompt::Blocks(vec![
                prefix_uncached,
                SystemBlock {
                    block_type: "text".to_string(),
                    text: s,
                    cache_control: Some(CacheControl::ephemeral_1h()),
                },
            ])),
            None => Some(SystemPrompt::Blocks(vec![SystemBlock {
                block_type: "text".to_string(),
                text: "You are Claude Code, Anthropic's official CLI for Claude.".to_string(),
                cache_control: Some(CacheControl::ephemeral_1h()),
            }])),
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

    fn mark_message_cacheable(message: &mut NativeMessage) -> bool {
        if let Some(last_content) = message.content.last_mut() {
            match last_content {
                NativeContentOut::Text { cache_control, .. }
                | NativeContentOut::ToolResult { cache_control, .. } => {
                    *cache_control = Some(CacheControl::ephemeral());
                    return true;
                }
                NativeContentOut::ToolUse { .. }
                | NativeContentOut::Image { .. }
                | NativeContentOut::Thinking { .. } => {}
            }
        }
        false
    }

    fn apply_cache_to_last_message(messages: &mut [NativeMessage]) {
        let len = messages.len();
        if len == 0 {
            return;
        }
        let mut marked = 0usize;
        if Self::mark_message_cacheable(&mut messages[len - 1]) {
            marked += 1;
        }
        if marked > 0 && len > 6 {
            let anchor = len - 5;
            for idx in (0..=anchor).rev() {
                if Self::mark_message_cacheable(&mut messages[idx]) {
                    break;
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
            last_tool.cache_control = Some(CacheControl::ephemeral_1h());
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
            .filter(|t| !t.trim().is_empty())?;
        let signature = msg
            .metadata
            .get("thinking_signature")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())?;
        Some(NativeContentOut::Thinking {
            thinking: thinking.to_string(),
            signature: signature.to_string(),
        })
    }

    fn thinking_block_from_envelope(value: &serde_json::Value) -> Option<NativeContentOut> {
        let thinking = value
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .filter(|t| !t.trim().is_empty())?;
        let signature = value
            .get("thinking_signature")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())?;
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
            let Some(tool_use_id) = v
                .get("tool_use_id")
                .and_then(|t| t.as_str())
                .or_else(|| v.get("tool_call_id").and_then(|t| t.as_str()))
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
            else {
                continue;
            };
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
        let mut native_messages: Vec<NativeMessage> = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    if msg.content.trim().is_empty() {
                        continue;
                    }
                    if native_messages.is_empty() {
                        system_parts.push(msg.content.clone());
                        continue;
                    }
                    let note = NativeMessage {
                        role: "user".to_string(),
                        content: vec![NativeContentOut::Text {
                            text: format!("[system-note]\n{}", msg.content),
                            cache_control: None,
                        }],
                    };
                    let same_role = native_messages.last().is_some_and(|m| m.role == "user");
                    if let (true, Some(last)) = (same_role, native_messages.last_mut()) {
                        last.content.extend(note.content);
                    } else {
                        native_messages.push(note);
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
                cache_control: Some(CacheControl::ephemeral_1h()),
            }]))
        };

        (system_prompt, Self::merge_adjacent_same_role_native(native_messages))
    }

    fn merge_adjacent_same_role_native(messages: Vec<NativeMessage>) -> Vec<NativeMessage> {
        let mut out: Vec<NativeMessage> = Vec::with_capacity(messages.len());
        for msg in messages {
            if let Some(last) = out.last_mut() {
                if last.role == msg.role {
                    let next_has_thinking = msg
                        .content
                        .iter()
                        .any(|b| matches!(b, NativeContentOut::Thinking { .. }));
                    if !next_has_thinking {
                        last.content.extend(msg.content);
                        continue;
                    }
                }
            }
            out.push(msg);
        }
        out
    }

    fn parse_native_response(response: NativeChatResponse) -> ProviderChatResponse {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut thinking_parts: Vec<String> = Vec::new();
        let mut signed_thinking: Option<(String, String)> = None;
        let stop_reason = response
            .stop_reason
            .as_deref()
            .and_then(crate::providers::traits::StopReason::from_wire);

        let usage = response.usage.map(|u| {
            let cached = u.cache_read_input_tokens;
            let created = u.cache_creation_input_tokens;
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, created);
            TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: created,
                reasoning_tokens: None,
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
                        if let Some(sig) = block.signature.filter(|s| !s.is_empty()) {
                            signed_thinking = Some((t, sig));
                        } else {
                            thinking_parts.push(t);
                        }
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

        let (reasoning_content, thinking_signature) = match signed_thinking {
            Some((text, sig)) => (Some(text), Some(sig)),
            None if !thinking_parts.is_empty() => (Some(thinking_parts.join("\n")), None),
            None => (None, None),
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
            thinking_signature,
            stop_reason,
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
        cancel_token: Option<tokio_util::sync::CancellationToken>,
    ) {
        let mut byte_stream = response.bytes_stream();
        let mut parser = crate::providers::core::sse::SseParser::new();

        let mut tool_id: Option<String> = None;
        let mut tool_name: Option<String> = None;
        let mut tool_input_json = String::new();
        let mut tool_args_overflow = false;
        let mut total_tool_args_bytes: usize = 0;
        let mut made_progress = false;
        let mut saw_stop_reason = false;
        let mut usage_acc = AnthropicStreamUsage::default();
        let mut thinking_sig_buf = String::new();
        let mut thinking_sig_overflow = false;

        let mut stream_ended = false;
        while !stream_ended {
            let next_bytes = tokio::select! {
                _ = crate::providers::stream_cancelled(&cancel_token) => return,
                next = byte_stream.next() => next,
            };
            match next_bytes {
                Some(Ok(bytes)) => {
                    parser.push(&bytes);
                    if parser.overflowed() {
                        let _ = tx
                            .send(Err(StreamError::Provider(
                                "anthropic SSE event exceeded size limit; upstream response malformed or truncated".to_string(),
                            )))
                            .await;
                        return;
                    }
                }
                Some(Err(e)) => {
                    let _ = tx
                        .send(Err(StreamError::Provider(format!(
                            "anthropic stream read failed before completion: {e}"
                        ))))
                        .await;
                    return;
                }
                None => {
                    parser.finish();
                    stream_ended = true;
                }
            }

            while let Some(sse_event) = parser.next_event() {
                if sse_event.data.trim().is_empty() || sse_event.is_done() {
                    continue;
                }
                let event: serde_json::Value = match serde_json::from_str(&sse_event.data) {
                    Ok(v) => v,
                    Err(_) => {
                        tracing::debug!("Skipping malformed SSE event: {}", sse_event.data);
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
                        if let Some(reason) = event
                            .get("delta")
                            .and_then(|d| d.get("stop_reason"))
                            .and_then(|r| r.as_str())
                            .and_then(crate::providers::traits::StopReason::from_wire)
                        {
                            saw_stop_reason = true;
                            let _ = tx.send(Ok(StreamEvent::StopReason(reason))).await;
                        }
                    }
                    "content_block_start" => {
                        if let Some(block) = event.get("content_block") {
                            let block_type = block
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or_default();
                            if block_type == "tool_use" {
                                if tool_id.is_some()
                                    || tool_name.is_some()
                                    || !tool_input_json.is_empty()
                                {
                                    let name = tool_name.take().unwrap_or_default();
                                    if tool_args_overflow {
                                        let _ = tx
                                            .send(Err(StreamError::Provider(format!(
                                                "anthropic tool_use `{name}` input_json exceeded the stream size limit; truncated arguments are not valid JSON (fail-closed)"
                                            ))))
                                            .await;
                                        return;
                                    } else if !name.trim().is_empty() {
                                        let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                                            tool_id.take(),
                                            crate::providers::sanitize::ProviderKind::Anthropic,
                                        );
                                        let input = std::mem::take(&mut tool_input_json);
                                        let safe_input =
                                            crate::providers::sanitize::normalize_tool_call_arguments(
                                                &name, input,
                                            );
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
                                tool_id = block
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .map(ToString::to_string);
                                tool_name = block
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .map(ToString::to_string);
                                tool_input_json.clear();
                                tool_args_overflow = false;
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
                                        let per_call_room =
                                            crate::providers::core::openai_sse::MAX_STREAM_TOOL_ARGS_BYTES
                                                .saturating_sub(tool_input_json.len());
                                        let total_room =
                                            crate::providers::core::openai_sse::MAX_STREAM_TOOL_ARGS_TOTAL_BYTES
                                                .saturating_sub(total_tool_args_bytes);
                                        let room = per_call_room.min(total_room);
                                        if json.len() <= room {
                                            tool_input_json.push_str(json);
                                            total_tool_args_bytes =
                                                total_tool_args_bytes.saturating_add(json.len());
                                        } else if !tool_args_overflow {
                                            tool_args_overflow = true;
                                            tracing::warn!(
                                                target: "providers.anthropic.stream",
                                                per_call_limit = crate::providers::core::openai_sse::MAX_STREAM_TOOL_ARGS_BYTES,
                                                total_limit = crate::providers::core::openai_sse::MAX_STREAM_TOOL_ARGS_TOTAL_BYTES,
                                                "anthropic tool_use input_json exceeded size limit; truncating"
                                            );
                                        }
                                    }
                                }
                                "signature_delta" => {
                                    if let Some(sig) =
                                        delta.get("signature").and_then(|s| s.as_str())
                                    {
                                        let room =
                                            crate::providers::core::openai_sse::MAX_STREAM_TOOL_ARGS_BYTES
                                                .saturating_sub(thinking_sig_buf.len());
                                        if sig.len() <= room {
                                            thinking_sig_buf.push_str(sig);
                                        } else if !thinking_sig_overflow {
                                            thinking_sig_overflow = true;
                                            tracing::warn!(
                                                target: "providers.anthropic.stream",
                                                limit = crate::providers::core::openai_sse::MAX_STREAM_TOOL_ARGS_BYTES,
                                                "anthropic thinking signature exceeded size limit; truncating"
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "content_block_stop" => {
                        if std::mem::take(&mut thinking_sig_overflow) {
                            thinking_sig_buf.clear();
                            tracing::warn!(
                                target: "providers.anthropic.stream",
                                "dropping truncated thinking signature; a truncated signature is cryptographically invalid and would be rejected when replayed to the API"
                            );
                        } else if !thinking_sig_buf.is_empty() {
                            let sig = std::mem::take(&mut thinking_sig_buf);
                            if tx
                                .send(Ok(StreamEvent::ReasoningSignature(sig)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        if tool_id.is_some() || tool_name.is_some() || !tool_input_json.is_empty()
                        {
                            let name = tool_name.take().unwrap_or_default();
                            if std::mem::take(&mut tool_args_overflow) {
                                let _ = tx
                                    .send(Err(StreamError::Provider(format!(
                                        "anthropic tool_use `{name}` input_json exceeded the stream size limit; truncated arguments are not valid JSON (fail-closed)"
                                    ))))
                                    .await;
                                return;
                            } else if name.trim().is_empty() {
                                tool_id = None;
                                tool_input_json.clear();
                            } else {
                                let id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                                    tool_id.take(),
                                    crate::providers::sanitize::ProviderKind::Anthropic,
                                );
                                let input = std::mem::take(&mut tool_input_json);
                                let safe_input =
                                    crate::providers::sanitize::normalize_tool_call_arguments(
                                        &name, input,
                                    );
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
                    }
                    "message_stop" => {
                        if std::mem::take(&mut tool_args_overflow) {
                            let name = tool_name.take().unwrap_or_default();
                            let _ = tx
                                .send(Err(StreamError::Provider(format!(
                                    "anthropic tool_use `{name}` input_json exceeded the stream size limit; truncated arguments are not valid JSON (fail-closed)"
                                ))))
                                .await;
                            return;
                        }
                        flush_pending_tool_call(
                            &mut tool_id,
                            &mut tool_name,
                            &mut tool_input_json,
                            tx,
                        )
                        .await;
                        if let Some(usage) = usage_acc.into_token_usage() {
                            let _ = tx.send(Ok(StreamEvent::Usage(usage))).await;
                        }
                        let _ = tx.send(Ok(StreamEvent::Final)).await;
                        return;
                    }
                    "error" => {
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
        }

        if tool_id.is_some() || tool_name.is_some() || !tool_input_json.is_empty() {
            let _ = tx
                .send(Err(StreamError::Provider(
                    "anthropic stream ended before message_stop while a tool_use block was still streaming; connection closed mid-response".to_string(),
                )))
                .await;
            return;
        }
        if !made_progress && !saw_stop_reason {
            let _ = tx
                .send(Err(StreamError::Provider(
                    "anthropic stream reached EOF without message_stop and without any text/tool output; connection closed mid-response (truncated)".to_string(),
                )))
                .await;
            return;
        }
        if !made_progress && saw_stop_reason {
            tracing::warn!(
                target: "providers.anthropic.stream",
                "anthropic stream reported a stop_reason but produced no text/tool output; finishing with an empty response"
            );
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
            reasoning_tokens: None,
        })
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
    let safe_input = crate::providers::sanitize::normalize_tool_call_arguments(&name, input);
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

        let (max_tokens, temperature, thinking) = self.request_tuning(model, temperature);
        let request = NativeChatRequest {
            model: model.to_string(),
            max_tokens,
            system,
            messages: vec![NativeMessage {
                role: "user".to_string(),
                content: vec![NativeContentOut::Text {
                    text: message.to_string(),
                    cache_control: None,
                }],
            }],
            temperature,
            thinking,
            tools: None,
            tool_choice: None,
            stream: None,
        };

        let mut request = crate::providers::core::idempotency::apply_idempotency_header(
            self.http_client()
                .post(format!("{}/v1/messages", self.base_url)),
        )
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
        if let Some(u) = parsed.usage.as_ref() {
            crate::providers::record_text_path_usage(
                "anthropic",
                model,
                u.input_tokens,
                u.output_tokens,
                u.cached_input_tokens,
            );
        }
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

        let tools_reserve = request
            .tools
            .map(crate::providers::traits::estimate_tool_specs_tokens)
            .unwrap_or(0);
        let messages_owned = std::sync::Arc::new(
            crate::providers::sanitize::sanitize_messages_before_send_for_provider(
                request.messages.to_vec(),
                model,
                (self.effective_max_tokens(model) as usize).saturating_add(tools_reserve),
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
        let (max_tokens, mut tuned_temperature, mut thinking) =
            self.request_tuning(model, temperature);

        let forces_tool_use = tool_choice
            .as_ref()
            .and_then(|tc| tc.get("type"))
            .and_then(|t| t.as_str())
            .is_some_and(|t| t != "auto");
        let mut messages = messages;
        if forces_tool_use && thinking.is_some() {
            thinking = None;
            tuned_temperature = temperature;
            for msg in messages.iter_mut() {
                msg.content
                    .retain(|block| !matches!(block, NativeContentOut::Thinking { .. }));
            }
            messages.retain(|m| !m.content.is_empty());
        }

        let native_request = NativeChatRequest {
            model: model.to_string(),
            max_tokens,
            system: system_prompt,
            messages,
            temperature: tuned_temperature,
            thinking,
            tools: native_tools,
            tool_choice,
            stream: None,
        };

        let req = crate::providers::core::idempotency::apply_idempotency_header(
            self.http_client()
                .post(format!("{}/v1/messages", self.base_url)),
        )
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
            parameters: crate::tools::schema::SchemaCleanr::clean_for_anthropic(schema.clone()),
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

        let tools_reserve = request
            .tools
            .map(crate::providers::traits::estimate_tool_specs_tokens)
            .unwrap_or(0);
        let sanitized_messages =
            crate::providers::sanitize::sanitize_messages_before_send_for_provider(
                request.messages.to_vec(),
                model,
                (self.effective_max_tokens(model) as usize).saturating_add(tools_reserve),
                None,
                crate::providers::sanitize::ProviderKind::Anthropic,
            );

        let tool_choice_override = crate::agent::loop_::TOOL_CHOICE_OVERRIDE
            .try_with(Clone::clone)
            .ok()
            .flatten();
        let request_tools: Option<Vec<ToolSpec>> = request.tools.map(<[ToolSpec]>::to_vec);

        let model_owned = model.to_string();
        let requested_temperature = temperature;
        let (max_tokens, temperature, thinking) = self.request_tuning(model, temperature);
        let client = self.stream_http_client();
        let url = format!("{}/v1/messages", self.base_url);
        let is_oauth = Self::is_setup_token(&credential);
        let reasoning_enabled = self.reasoning_enabled;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(64);
        let idempotency_key = crate::providers::core::idempotency::current_idempotency_key();
        let cancel_token = crate::providers::current_session_cancel_token();

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

            let mut temperature = temperature;
            let mut thinking = thinking;
            let mut messages = messages;
            let forces_tool_use = tool_choice
                .as_ref()
                .and_then(|tc| tc.get("type"))
                .and_then(|t| t.as_str())
                .is_some_and(|t| t != "auto");
            if forces_tool_use && thinking.is_some() {
                thinking = None;
                temperature = requested_temperature;
                for msg in messages.iter_mut() {
                    msg.content
                        .retain(|block| !matches!(block, NativeContentOut::Thinking { .. }));
                }
                messages.retain(|m| !m.content.is_empty());
            }

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
                thinking,
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

            let mut req = crate::providers::core::idempotency::apply_idempotency_header_value(
                client.post(&url),
                idempotency_key,
            )
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body);

            if is_oauth {
                req = req
                    .header("Authorization", format!("Bearer {credential}"))
                    .header(
                        "anthropic-beta",
                        "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,extended-cache-ttl-2025-04-11",
                    )
                    .header("anthropic-dangerous-direct-browser-access", "true");
            } else {
                req = req.header("x-api-key", &credential);
                let beta = if reasoning_enabled {
                    "fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14,extended-cache-ttl-2025-04-11"
                } else {
                    "fine-grained-tool-streaming-2025-05-14,extended-cache-ttl-2025-04-11"
                };
                req = req.header("anthropic-beta", beta);
            }

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(StreamError::Http(e))).await;
                    return;
                }
            };

            if !response.status().is_success() {
                let (status, error) =
                    super::stream_error_body_with_retry_after(response).await;
                let sanitized = super::sanitize_api_error(&error);
                let _ = tx
                    .send(Err(StreamError::Provider(format!("{status}: {sanitized}"))))
                    .await;
                return;
            }

            Self::parse_anthropic_sse(response, &tx, cancel_token).await;
        });

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })
        .boxed()
    }
}
