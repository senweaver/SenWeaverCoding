// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::tools::ToolSpec;
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,

    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

pub const EPHEMERAL_CONTEXT_KEY: &str = "ephemeral_context";
pub const TURN_COMPANION_KEY: &str = "turn_companion";

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            metadata: Default::default(),
        }
    }

    #[must_use]
    pub fn with_ephemeral_context(mut self, wire_content: impl Into<String>) -> Self {
        let wire = wire_content.into();
        if !wire.is_empty() && wire != self.content {
            self.metadata.insert(
                EPHEMERAL_CONTEXT_KEY.to_string(),
                serde_json::Value::String(wire),
            );
        }
        self
    }

    #[must_use]
    pub fn with_turn_companion(mut self, companion: impl Into<String>) -> Self {
        let text = companion.into();
        if !text.trim().is_empty() {
            self.metadata.insert(
                TURN_COMPANION_KEY.to_string(),
                serde_json::Value::String(text),
            );
        }
        self
    }

    #[must_use]
    pub fn turn_companion(&self) -> Option<&str> {
        self.metadata
            .get(TURN_COMPANION_KEY)
            .and_then(|v| v.as_str())
    }

    #[must_use]
    pub fn has_current_request_marker(&self) -> bool {
        self.wire_content().contains("[CURRENT REQUEST")
            || self
                .turn_companion()
                .is_some_and(|c| c.contains("[CURRENT REQUEST"))
    }

    #[must_use]
    pub fn ephemeral_wire_content(&self) -> Option<&str> {
        self.metadata
            .get(EPHEMERAL_CONTEXT_KEY)
            .and_then(|v| v.as_str())
    }

    pub fn strip_ephemeral_context(&mut self) {
        self.metadata.remove(EPHEMERAL_CONTEXT_KEY);
    }

    #[must_use]
    pub fn wire_content(&self) -> &str {
        self.ephemeral_wire_content().unwrap_or(&self.content)
    }

    #[must_use]
    pub fn composed_for_send(&self) -> ChatMessage {
        match self.ephemeral_wire_content() {
            Some(wire) => {
                let mut composed = self.clone();
                composed.content = wire.to_string();
                composed.metadata.remove(EPHEMERAL_CONTEXT_KEY);
                composed
            }
            None => self.clone(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            metadata: Default::default(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            metadata: Default::default(),
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            metadata: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,

    pub cached_input_tokens: Option<u64>,

    pub cache_creation_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ChatResponse {

    pub text: Option<String>,

    pub tool_calls: Vec<ToolCall>,

    pub usage: Option<TokenUsage>,

    pub reasoning_content: Option<String>,

    pub thinking_signature: Option<String>,

    pub stop_reason: Option<StopReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
}

impl StopReason {
    pub fn from_wire(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "stop" | "end_turn" | "stop_sequence" | "eos" | "done" | "complete" => {
                Some(Self::Stop)
            }
            "length" | "max_tokens" | "max_output_tokens" | "model_length" => Some(Self::Length),
            "tool_calls" | "tool_use" | "function_call" => Some(Self::ToolCalls),
            "content_filter" | "content_filtered" | "guardrail_intervened" | "safety"
            | "recitation" | "blocklist" | "prohibited_content" | "spii" => {
                Some(Self::ContentFilter)
            }
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ToolCalls => "tool_calls",
            Self::ContentFilter => "content_filter",
        }
    }
}

impl ChatResponse {

    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    pub fn text_or_empty(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    pub fn truncated_by_length(&self) -> bool {
        matches!(self.stop_reason, Some(StopReason::Length))
    }

    pub fn text_only(text: impl Into<Option<String>>, usage: Option<TokenUsage>) -> Self {
        Self {
            text: text.into(),
            tool_calls: Vec::new(),
            usage,
            reasoning_content: None,
            thinking_signature: None,
            stop_reason: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChatRequest<'a> {
    pub messages: &'a [ChatMessage],
    pub tools: Option<&'a [ToolSpec]>,
}

#[derive(Debug, Clone)]
pub struct StructuredResponse {

    pub data: serde_json::Value,

    pub raw_text: String,

    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ConversationMessage {

    Chat(ChatMessage),

    AssistantToolCalls {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,

        reasoning_content: Option<String>,
    },

    ToolResults(Vec<ToolResultMessage>),
}

#[derive(Debug, Clone)]
pub struct StreamChunk {

    pub delta: String,

    pub reasoning: Option<String>,

    pub is_final: bool,

    pub token_count: usize,
}

impl StreamChunk {

    pub fn delta(text: impl Into<String>) -> Self {
        Self {
            delta: text.into(),
            reasoning: None,
            is_final: false,
            token_count: 0,
        }
    }

    pub fn reasoning(text: impl Into<String>) -> Self {
        Self {
            delta: String::new(),
            reasoning: Some(text.into()),
            is_final: false,
            token_count: 0,
        }
    }

    pub fn final_chunk() -> Self {
        Self {
            delta: String::new(),
            reasoning: None,
            is_final: true,
            token_count: 0,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            delta: message.into(),
            reasoning: None,
            is_final: true,
            token_count: 0,
        }
    }

    pub fn with_token_estimate(mut self) -> Self {
        self.token_count = estimate_content_tokens(&self.delta).max(1);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    EngineOverloaded,
    AccountRateLimited,
    Transient,
}

impl RetryClass {
    pub fn as_str(self) -> &'static str {
        match self {
            RetryClass::EngineOverloaded => "engine_overloaded",
            RetryClass::AccountRateLimited => "account_rate_limited",
            RetryClass::Transient => "transient",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetryNotice {
    pub attempt: u32,
    pub max_attempts: u32,
    pub wait_ms: u64,
    pub failure_class: RetryClass,
    pub provider: String,
    pub model: String,
    pub last_error_summary: String,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {

    TextDelta(StreamChunk),

    ToolCall(ToolCall),

    PreExecutedToolCall { name: String, args: String },

    PreExecutedToolResult { name: String, output: String },

    Usage(TokenUsage),

    ReasoningSignature(String),

    StopReason(StopReason),

    Retry(RetryNotice),

    Final,
}

impl StreamEvent {
    pub(crate) fn from_chunk(chunk: StreamChunk) -> Self {
        if chunk.is_final {
            Self::Final
        } else {
            Self::TextDelta(chunk)
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StreamOptions {

    pub enabled: bool,

    pub count_tokens: bool,
}

impl StreamOptions {

    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            count_tokens: false,
        }
    }

    pub fn with_token_count(mut self) -> Self {
        self.count_tokens = true;
        self
    }
}

pub type StreamResult<T> = std::result::Result<T, StreamError>;

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("HTTP error: {0}")]
    Http(reqwest::Error),

    #[error("JSON parse error: {0}")]
    Json(serde_json::Error),

    #[error("Invalid SSE format: {0}")]
    InvalidSse(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl crate::error::ErrorClassification for StreamError {
    fn category(&self) -> crate::error::ErrorCategory {
        use crate::error::ErrorCategory;
        match self {
            StreamError::Http(e) => {
                if e.is_timeout() {
                    ErrorCategory::Timeout
                } else if e.is_connect() || e.is_request() {
                    ErrorCategory::Network
                } else if let Some(status) = e.status() {
                    let code = status.as_u16();
                    if code == 429 {
                        ErrorCategory::RateLimit
                    } else if code == 401 || code == 403 {
                        ErrorCategory::Permission
                    } else if code == 404 {
                        ErrorCategory::NotFound
                    } else if (500..600).contains(&code) {
                        ErrorCategory::Provider
                    } else if (400..500).contains(&code) {
                        ErrorCategory::Validation
                    } else {
                        ErrorCategory::Provider
                    }
                } else {
                    ErrorCategory::Provider
                }
            }
            StreamError::Json(_) | StreamError::InvalidSse(_) => ErrorCategory::Provider,
            StreamError::Provider(_) => ErrorCategory::Provider,
            StreamError::Io(e) => crate::error::ErrorClassification::category(e),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("provider_capability_error provider={provider} capability={capability} message={message}")]
pub struct ProviderCapabilityError {
    pub provider: String,
    pub capability: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderCapabilities {

    pub native_tool_calling: bool,

    pub vision: bool,

    pub prompt_caching: bool,

    pub responses_api: bool,
}

#[derive(Debug, Clone)]
pub enum ToolsPayload {

    Gemini {
        function_declarations: Vec<serde_json::Value>,
    },

    Anthropic { tools: Vec<serde_json::Value> },

    OpenAI { tools: Vec<serde_json::Value> },

    PromptGuided { instructions: String },
}

#[async_trait]
pub trait Provider: Send + Sync {

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    fn message_format_kind(&self) -> crate::providers::sanitize::ProviderKind {
        crate::providers::sanitize::ProviderKind::OpenAi
    }

    fn consumes_reasoning_envelope(&self) -> bool {
        false
    }

    fn convert_tools(&self, tools: &[ToolSpec]) -> ToolsPayload {
        ToolsPayload::PromptGuided {
            instructions: build_tool_instructions_text(tools),
        }
    }

    async fn simple_chat(
        &self,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        self.chat_with_system(None, message, model, temperature)
            .await
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String>;

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.as_str());
        let conversation = format_conversation_history(messages);
        self.chat_with_system(system, &conversation, model, temperature)
            .await
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {

        if let Some(tools) = request.tools {
            if !tools.is_empty() && !self.supports_native_tools() {
                let tool_instructions = match self.convert_tools(tools) {
                    ToolsPayload::PromptGuided { instructions } => instructions,
                    payload => {
                        anyhow::bail!(
                            "Provider returned non-prompt-guided tools payload ({payload:?}) while supports_native_tools() is false"
                        )
                    }
                };
                let mut modified_messages = request.messages.to_vec();

                if let Some(system_message) =
                    modified_messages.iter_mut().find(|m| m.role == "system")
                {
                    if !system_message.content.is_empty() {
                        system_message.content.push_str("\n\n");
                    }
                    system_message.content.push_str(&tool_instructions);
                } else {
                    modified_messages.insert(0, ChatMessage::system(tool_instructions));
                }

                let text = self
                    .chat_with_history(&modified_messages, model, temperature)
                    .await?;
                return Ok(ChatResponse::text_only(Some(text), None));
            }
        }

        let text = self
            .chat_with_history(request.messages, model, temperature)
            .await?;
        Ok(ChatResponse::text_only(Some(text), None))
    }

    fn supports_native_tools(&self) -> bool {
        self.capabilities().native_tool_calling
    }

    fn supports_vision(&self) -> bool {
        self.capabilities().vision
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let text = self.chat_with_history(messages, model, temperature).await?;
        Ok(ChatResponse::text_only(Some(text), None))
    }

    async fn chat_structured(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<StructuredResponse> {
        let mut augmented: Vec<ChatMessage> = messages.to_vec();
        let schema_text = serde_json::to_string_pretty(schema)
            .unwrap_or_else(|_| schema.to_string());
        let instruction = format!(
            "Reply with a single JSON object that conforms to the schema below. \
             Do not include explanations, prose, or Markdown fences -- only the raw JSON.\n\n\
             [JSON Schema]\n{schema_text}"
        );
        if let Some(system) = augmented.iter_mut().find(|m| m.role == "system") {
            if !system.content.is_empty() {
                system.content.push_str("\n\n");
            }
            system.content.push_str(&instruction);
        } else {
            augmented.insert(0, ChatMessage::system(instruction));
        }
        let raw = self
            .chat_with_history(&augmented, model, temperature)
            .await?;
        let parsed = parse_first_json_object(&raw).ok_or_else(|| {
            anyhow::anyhow!(
                "Provider returned no JSON object after schema-constrained chat: {raw}"
            )
        })?;
        Ok(StructuredResponse {
            data: parsed,
            raw_text: raw,
            usage: None,
        })
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_streaming_tool_events(&self) -> bool {
        false
    }

    fn stream_chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
        _options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {

        stream::once(async {
            Err(StreamError::Provider(
                "streaming is not implemented by this provider; check supports_streaming() \
                 before requesting a stream"
                    .to_string(),
            ))
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
        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.as_str());
        let conversation = format_conversation_history(messages);
        self.stream_chat_with_system(system, &conversation, model, temperature, options)
    }

    fn stream_chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
        self.stream_chat_with_history(request.messages, model, temperature, options)
            .map(|chunk_result| chunk_result.map(StreamEvent::from_chunk))
            .boxed()
    }
}

pub fn build_tool_instructions_text(tools: &[ToolSpec]) -> String {
    let mut instructions = String::new();

    instructions.push_str("## Tool Use Protocol\n\n");
    instructions.push_str("To use a tool, wrap a JSON object in <tool_call></tool_call> tags:\n\n");
    instructions.push_str("<tool_call>\n");
    instructions.push_str(r#"{"name": "tool_name", "arguments": {"param": "value"}}"#);
    instructions.push_str("\n</tool_call>\n\n");
    instructions.push_str("You may use multiple tool calls in a single response. ");
    instructions.push_str("After tool execution, results appear in <tool_result> tags. ");
    instructions
        .push_str("Continue reasoning with the results until you can give a final answer.\n\n");
    instructions.push_str("### Available Tools\n\n");

    for tool in tools {
        writeln!(&mut instructions, "**{}**: {}", tool.name, tool.description)
            .expect("writing to String cannot fail");

        let parameters =
            serde_json::to_string(&tool.parameters).unwrap_or_else(|_| "{}".to_string());
        writeln!(&mut instructions, "Parameters: `{parameters}`")
            .expect("writing to String cannot fail");
        instructions.push('\n');
    }

    instructions
}

pub fn estimate_message_tokens(message: &ChatMessage) -> usize {
    let role_overhead = 4_usize;
    let mut content_tokens = estimate_content_tokens(message.wire_content());
    if let Some(companion) = message.turn_companion() {
        content_tokens = content_tokens
            .saturating_add(role_overhead)
            .saturating_add(estimate_content_tokens(companion));
    }
    role_overhead.saturating_add(content_tokens)
}

pub fn estimate_content_tokens(content: &str) -> usize {
    const MEMO_MIN_LEN: usize = 2048;
    const MEMO_CAP: usize = 4096;
    if content.len() < MEMO_MIN_LEN {
        return estimate_content_tokens_uncached(content);
    }
    use std::hash::{Hash, Hasher};
    static MEMO: std::sync::LazyLock<
        parking_lot::Mutex<std::collections::HashMap<u64, usize>>,
    > = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    let key = hasher.finish();
    if let Some(cached) = MEMO.lock().get(&key) {
        return *cached;
    }
    let estimate = estimate_content_tokens_uncached(content);
    let mut memo = MEMO.lock();
    if memo.len() >= MEMO_CAP {
        memo.clear();
    }
    memo.insert(key, estimate);
    estimate
}

fn estimate_content_tokens_uncached(content: &str) -> usize {
    let mut ascii_chars = 0_usize;
    let mut wide_chars = 0_usize;
    for ch in content.chars() {
        if ch.is_ascii() {
            ascii_chars += 1;
        } else {
            wide_chars += 1;
        }
    }
    ascii_chars
        .saturating_mul(10)
        .div_ceil(34)
        .saturating_add(wide_chars)
}

pub fn hash_json_value<H: std::hash::Hasher>(
    value: &serde_json::Value,
    hasher: &mut H,
    depth: usize,
) {
    use std::hash::Hash;
    if depth >= 96 {
        0xdeu8.hash(hasher);
        return;
    }
    match value {
        serde_json::Value::Null => 0u8.hash(hasher),
        serde_json::Value::Bool(b) => {
            1u8.hash(hasher);
            b.hash(hasher);
        }
        serde_json::Value::Number(n) => {
            2u8.hash(hasher);
            n.to_string().hash(hasher);
        }
        serde_json::Value::String(s) => {
            3u8.hash(hasher);
            s.hash(hasher);
        }
        serde_json::Value::Array(arr) => {
            4u8.hash(hasher);
            arr.len().hash(hasher);
            for v in arr {
                hash_json_value(v, hasher, depth + 1);
            }
        }
        serde_json::Value::Object(map) => {
            5u8.hash(hasher);
            map.len().hash(hasher);
            for (k, v) in map {
                k.hash(hasher);
                hash_json_value(v, hasher, depth + 1);
            }
        }
    }
}

pub fn estimate_tool_specs_tokens(tools: &[crate::tools::ToolSpec]) -> usize {
    use std::hash::{Hash, Hasher};
    const MAX_CACHED_TOOLSETS: usize = 32;
    static CACHE: std::sync::LazyLock<
        parking_lot::Mutex<std::collections::HashMap<u64, usize>>,
    > = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));
    if tools.is_empty() {
        return 0;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tools.len().hash(&mut hasher);
    for tool in tools {
        tool.name.hash(&mut hasher);
        tool.description.hash(&mut hasher);
        hash_json_value(&tool.parameters, &mut hasher, 0);
    }
    let key = hasher.finish();
    if let Some(cached_value) = CACHE.lock().get(&key) {
        return *cached_value;
    }
    let estimate = tools
        .iter()
        .map(|tool| {
            16_usize
                .saturating_add(estimate_content_tokens(&tool.name))
                .saturating_add(estimate_content_tokens(&tool.description))
                .saturating_add(estimate_content_tokens(
                    &serde_json::to_string(&tool.parameters).unwrap_or_default(),
                ))
        })
        .sum();
    let mut cache = CACHE.lock();
    if cache.len() >= MAX_CACHED_TOOLSETS {
        cache.clear();
    }
    cache.insert(key, estimate);
    estimate
}

fn format_conversation_history(messages: &[ChatMessage]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for msg in messages {
        if msg.role == "system" {
            continue;
        }
        let role_label = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "tool" => "Tool",
            other => other,
        };
        parts.push(format!("{role_label}: {}", msg.content));
    }
    parts.join("\n\n")
}

pub fn estimate_total_tokens(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(estimate_message_tokens)
        .sum::<usize>()
        .saturating_add(8)
}

pub fn enforce_context_budget_native(
    messages: Vec<ChatMessage>,
    model: &str,
    reserve_output_tokens: usize,
) -> Vec<ChatMessage> {
    enforce_context_budget_native_with_window(messages, model, reserve_output_tokens, None)
}

pub fn enforce_context_budget_native_with_window(
    messages: Vec<ChatMessage>,
    model: &str,
    reserve_output_tokens: usize,
    context_window_override: Option<usize>,
) -> Vec<ChatMessage> {
    let window = context_window_override
        .unwrap_or_else(|| crate::constants::api_limits::context_window_for_model(model) as usize);
    let safety_margin = 256_usize;
    let reserve = reserve_output_tokens
        .saturating_add(safety_margin)
        .min(window.saturating_sub(512).max(512));
    let max_input = window.saturating_sub(reserve).max(512);
    let calibration = crate::agent::token::budget::calibration_factor_for(model).max(0.25) * 1.05;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_input = ((max_input as f64 / calibration).floor() as usize).max(512);

    let total = estimate_total_tokens(&messages);
    if total <= max_input {
        return messages;
    }

    let mut groups: Vec<Vec<ChatMessage>> = Vec::new();
    let mut leading_system: Vec<ChatMessage> = Vec::new();
    let mut current_group: Vec<ChatMessage> = Vec::new();
    let mut started_non_system = false;

    for msg in messages {
        if !started_non_system && msg.role == "system" {
            leading_system.push(msg);
            continue;
        }
        started_non_system = true;
        if msg.role == "tool" {
            if current_group.is_empty() {
                current_group.push(msg);
            } else {
                current_group.push(msg);
            }
            continue;
        }
        if !current_group.is_empty() {
            groups.push(std::mem::take(&mut current_group));
        }
        current_group.push(msg);
    }
    if !current_group.is_empty() {
        groups.push(current_group);
    }

    let last_group = groups.pop();
    let last_group_tokens = last_group
        .as_ref()
        .map(|g| g.iter().map(estimate_message_tokens).sum::<usize>())
        .unwrap_or(0);

    let mut system_tokens: usize = leading_system
        .iter()
        .map(estimate_message_tokens)
        .sum::<usize>();

    let mut available = max_input
        .saturating_sub(system_tokens)
        .saturating_sub(last_group_tokens);

    let total_groups = groups.len();
    let mut kept_groups: Vec<Vec<ChatMessage>> = Vec::new();
    let mut used: usize = 0;
    for group in groups.into_iter().rev() {
        let cost: usize = group.iter().map(estimate_message_tokens).sum();
        if used.saturating_add(cost) > available {
            break;
        }
        used = used.saturating_add(cost);
        kept_groups.push(group);
    }
    kept_groups.reverse();
    let dropped_groups = total_groups.saturating_sub(kept_groups.len());

    if system_tokens.saturating_add(last_group_tokens) > max_input {
        let target_for_system = max_input
            .saturating_sub(last_group_tokens)
            .saturating_sub(64)
            .max(256);
        truncate_system_messages(&mut leading_system, target_for_system);
        system_tokens = leading_system
            .iter()
            .map(estimate_message_tokens)
            .sum::<usize>();
        available = max_input
            .saturating_sub(system_tokens)
            .saturating_sub(last_group_tokens);
        if used > available {
            kept_groups.clear();
        }
    }

    let mut out: Vec<ChatMessage> = leading_system;
    if dropped_groups > 0 {
        out.push(ChatMessage::system(format!(
            "[Context trimmed: {dropped_groups} older conversation turn(s) were dropped to fit \
             the model context window. Earlier history is incomplete; ask the user to restate \
             anything you need rather than assuming it was never said.]"
        )));
    }
    for group in kept_groups {
        out.extend(group);
    }
    if let Some(group) = last_group {
        out.extend(group);
    }
    out
}

pub fn enforce_context_budget(
    messages: Vec<ChatMessage>,
    model: &str,
    reserve_output_tokens: usize,
) -> Vec<ChatMessage> {
    enforce_context_budget_with_window(messages, model, reserve_output_tokens, None)
}

pub fn enforce_context_budget_with_window(
    messages: Vec<ChatMessage>,
    model: &str,
    reserve_output_tokens: usize,
    context_window_override: Option<usize>,
) -> Vec<ChatMessage> {
    enforce_context_budget_native_with_window(
        messages,
        model,
        reserve_output_tokens,
        context_window_override,
    )
}

fn truncate_system_messages(messages: &mut Vec<ChatMessage>, target_tokens: usize) {
    if messages.is_empty() {
        return;
    }
    let mut current: usize = messages
        .iter()
        .map(estimate_message_tokens)
        .sum::<usize>();
    if current <= target_tokens {
        return;
    }

    if messages.len() > 1 {
        while messages.len() > 1 && current > target_tokens {
            if let Some(removed) = messages.pop() {
                current = current.saturating_sub(estimate_message_tokens(&removed));
            } else {
                break;
            }
        }
        if current <= target_tokens {
            return;
        }
    }

    if let Some(first) = messages.first_mut() {
        let target_chars = target_tokens.saturating_mul(4).saturating_sub(64).max(256);
        if first.content.len() > target_chars {
            let head_chars = (target_chars * 2 / 3).max(128);
            let tail_chars = target_chars.saturating_sub(head_chars).saturating_sub(96);
            let head = safe_char_slice(&first.content, 0, head_chars);
            let tail = safe_char_slice_from_end(&first.content, tail_chars);
            first.content = format!(
                "{head}\n\n[...truncated to fit model context window...]\n\n{tail}"
            );
        }
    }
}

fn safe_char_slice(text: &str, start: usize, len: usize) -> String {
    text.chars().skip(start).take(len).collect()
}

fn safe_char_slice_from_end(text: &str, len: usize) -> String {
    let total = text.chars().count();
    let start = total.saturating_sub(len);
    text.chars().skip(start).collect()
}

pub fn flatten_messages_for_text_only_wire(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for msg in messages {
        match msg.role.as_str() {
            "tool" => {
                let (call_id, body) = parse_tool_envelope(&msg.content);
                let header = match call_id {
                    Some(id) if !id.is_empty() => format!("[Tool result for {id}]\n"),
                    _ => "[Tool result]\n".to_string(),
                };
                let formatted = format!("{header}{body}");
                if let Some(prev) = out.last_mut() {
                    if prev.role == "user" {
                        prev.content.push_str("\n\n");
                        prev.content.push_str(&formatted);
                        continue;
                    }
                }
                out.push(ChatMessage {
                    role: "user".to_string(),
                    content: formatted,
                    metadata: msg.metadata.clone(),
                });
            }
            "assistant" => {
                let flattened = flatten_assistant_envelope(&msg.content);
                out.push(ChatMessage {
                    role: msg.role.clone(),
                    content: flattened,
                    metadata: msg.metadata.clone(),
                });
            }
            _ => out.push(msg.clone()),
        }
    }
    super::sanitize::normalize_chat_messages_for_wire(out)
}

fn parse_tool_envelope(content: &str) -> (Option<String>, String) {
    let trimmed = content.trim_start();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return (None, content.to_string());
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        let id = value
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let body = value
            .get("content")
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_else(|| content.to_string());
        return (id, body);
    }
    (None, content.to_string())
}

fn flatten_assistant_envelope(content: &str) -> String {
    let trimmed = content.trim_start();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return content.to_string();
    }
    let value = match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v) => v,
        Err(_) => return content.to_string(),
    };
    let text_part = value
        .get("content")
        .and_then(|c| match c {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(arr) => {
                let mut joined = String::new();
                for item in arr {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        if !joined.is_empty() {
                            joined.push('\n');
                        }
                        joined.push_str(t);
                    }
                }
                if joined.is_empty() { None } else { Some(joined) }
            }
            _ => None,
        })
        .unwrap_or_default();

    let calls_part = value
        .get("tool_calls")
        .and_then(|c| c.as_array())
        .map(|arr| {
            let mut summary = String::new();
            for call in arr {
                let name = call
                    .get("name")
                    .and_then(|n| n.as_str())
                    .or_else(|| {
                        call.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                    })
                    .unwrap_or("unknown_tool");
                let args = call
                    .get("arguments")
                    .map(|a| match a {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .or_else(|| {
                        call.get("function")
                            .and_then(|f| f.get("arguments"))
                            .map(|a| match a {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            })
                    })
                    .unwrap_or_default();
                if !summary.is_empty() {
                    summary.push('\n');
                }
                summary.push_str(&format!("- {name}({args})"));
            }
            summary
        })
        .unwrap_or_default();

    if calls_part.is_empty() {
        if text_part.is_empty() {
            content.to_string()
        } else {
            text_part
        }
    } else if text_part.is_empty() {
        format!("[Tool calls]\n{calls_part}")
    } else {
        format!("{text_part}\n\n[Tool calls]\n{calls_part}")
    }
}

pub fn parse_first_json_object(text: &str) -> Option<serde_json::Value> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    let mut in_string = false;
    let mut escape = false;

    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        let slice = &text[s..=i];
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(slice) {
                            return Some(value);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}
