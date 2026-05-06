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

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            metadata: Default::default(),
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

#[derive(Debug, Clone)]
pub struct ChatResponse {

    pub text: Option<String>,

    pub tool_calls: Vec<ToolCall>,

    pub usage: Option<TokenUsage>,

    pub reasoning_content: Option<String>,
}

impl ChatResponse {

    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    pub fn text_or_empty(&self) -> &str {
        self.text.as_deref().unwrap_or("")
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
        self.token_count = self.delta.len().div_ceil(4);
        self
    }
}

#[derive(Debug, Clone)]
pub enum StreamEvent {

    TextDelta(StreamChunk),

    ToolCall(ToolCall),

    PreExecutedToolCall { name: String, args: String },

    PreExecutedToolResult { name: String, output: String },

    Usage(TokenUsage),

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
        let last_user = messages
            .iter()
            .rfind(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        self.chat_with_system(system, last_user, model, temperature)
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
                return Ok(ChatResponse {
                    text: Some(text),
                    tool_calls: Vec::new(),
                    usage: None,
                    reasoning_content: None,
                });
            }
        }

        let text = self
            .chat_with_history(request.messages, model, temperature)
            .await?;
        Ok(ChatResponse {
            text: Some(text),
            tool_calls: Vec::new(),
            usage: None,
            reasoning_content: None,
        })
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
        Ok(ChatResponse {
            text: Some(text),
            tool_calls: Vec::new(),
            usage: None,
            reasoning_content: None,
        })
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

        stream::empty().boxed()
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
        let last_user = messages
            .iter()
            .rfind(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        self.stream_chat_with_system(system, last_user, model, temperature, options)
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
