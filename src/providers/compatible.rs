// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::multimodal;
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, StreamChunk, StreamError, StreamEvent, StreamOptions, StreamResult, TokenUsage,
    ToolCall as ProviderToolCall,
};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use serde::{Deserialize, Serialize};

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    pub(crate) name: String,
    pub(crate) base_url: String,
    pub(crate) credential: Option<String>,
    pub(crate) auth_header: AuthStyle,
    supports_vision: bool,

    supports_responses_fallback: bool,
    user_agent: Option<String>,

    merge_system_into_user: bool,

    native_tool_calling: bool,

    timeout_secs: u64,

    extra_headers: std::collections::HashMap<String, String>,

    reasoning_effort: Option<String>,

    api_path: Option<String>,

    max_tokens: Option<u32>,

    model_context_windows: std::collections::HashMap<String, u32>,
}

#[derive(Debug, Clone)]
pub enum AuthStyle {

    Bearer,

    XApiKey,

    Custom(String),
}

impl OpenAiCompatibleProvider {
    pub fn new(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
    ) -> Self {
        Self::new_with_options(
            name, base_url, credential, auth_style, false, true, None, false,
        )
    }

    pub fn new_with_vision(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
        supports_vision: bool,
    ) -> Self {
        Self::new_with_options(
            name,
            base_url,
            credential,
            auth_style,
            supports_vision,
            true,
            None,
            false,
        )
    }

    pub fn new_no_responses_fallback(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
    ) -> Self {
        Self::new_with_options(
            name, base_url, credential, auth_style, false, false, None, false,
        )
    }

    pub fn new_with_user_agent(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
        user_agent: &str,
    ) -> Self {
        Self::new_with_options(
            name,
            base_url,
            credential,
            auth_style,
            false,
            true,
            Some(user_agent),
            false,
        )
    }

    pub fn new_with_user_agent_and_vision(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
        user_agent: &str,
        supports_vision: bool,
    ) -> Self {
        Self::new_with_options(
            name,
            base_url,
            credential,
            auth_style,
            supports_vision,
            true,
            Some(user_agent),
            false,
        )
    }

    pub fn new_merge_system_into_user(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
    ) -> Self {
        Self::new_with_options(
            name, base_url, credential, auth_style, false, false, None, true,
        )
    }

    fn new_with_options(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
        supports_vision: bool,
        supports_responses_fallback: bool,
        user_agent: Option<&str>,
        merge_system_into_user: bool,
    ) -> Self {
        Self {
            name: name.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            credential: credential.map(ToString::to_string),
            auth_header: auth_style,
            supports_vision,
            supports_responses_fallback,
            user_agent: user_agent.map(ToString::to_string),
            merge_system_into_user,
            native_tool_calling: !merge_system_into_user,
            timeout_secs: 120,
            extra_headers: std::collections::HashMap::new(),
            reasoning_effort: None,
            api_path: None,
            max_tokens: None,
            model_context_windows: std::collections::HashMap::new(),
        }
    }

    pub fn without_native_tools(mut self) -> Self {
        self.native_tool_calling = false;
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

    pub fn with_reasoning_effort(mut self, reasoning_effort: Option<String>) -> Self {
        self.reasoning_effort = reasoning_effort;
        self
    }

    pub fn with_api_path(mut self, api_path: Option<String>) -> Self {
        self.api_path = api_path;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_vision(mut self, supports_vision: bool) -> Self {
        self.supports_vision = supports_vision;
        self
    }

    pub fn with_model_context_windows(
        mut self,
        windows: std::collections::HashMap<String, u32>,
    ) -> Self {
        self.model_context_windows = windows;
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

    fn normalize_system_for_strict_wire(messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let leading_system_end = messages
            .iter()
            .position(|m| m.role != "system")
            .unwrap_or(messages.len());
        let has_trailing_system = messages
            .iter()
            .skip(leading_system_end)
            .any(|m| m.role == "system");
        if leading_system_end <= 1 && !has_trailing_system {
            return messages.to_vec();
        }
        let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
        if leading_system_end == 1 {
            out.push(messages[0].clone());
        } else if leading_system_end > 1 {
            let merged = messages[..leading_system_end]
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            out.push(ChatMessage::system(merged));
        }
        for m in &messages[leading_system_end..] {
            if m.role == "system" {
                out.push(ChatMessage::user(format!("[System note]\n{}", m.content)));
            } else {
                out.push(m.clone());
            }
        }
        out
    }

    fn flatten_system_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let system_content: String = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        if system_content.is_empty() {
            return messages.to_vec();
        }

        let mut result: Vec<ChatMessage> = messages
            .iter()
            .filter(|m| m.role != "system")
            .cloned()
            .collect();

        if let Some(first_user) = result.iter_mut().find(|m| m.role == "user") {
            first_user.content = format!("{system_content}\n\n{}", first_user.content);
        } else {

            result.insert(0, ChatMessage::user(&system_content));
        }

        result
    }

    fn http_client(&self) -> Client {
        let mut headers = self.extra_headers.clone();
        if let Some(ua) = self.user_agent.as_deref() {
            headers.insert("user-agent".to_string(), ua.to_string());
        }
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts_and_headers(
                "provider.compatible",
                self.timeout_secs,
                5,
                &headers,
            )
    }

    fn stream_http_client(&self) -> Client {
        let mut headers = HeaderMap::new();
        if let Some(ua) = self.user_agent.as_deref() {
            if let Ok(value) = HeaderValue::from_str(ua) {
                headers.insert(USER_AGENT, value);
            }
        }
        for (key, value) in &self.extra_headers {
            match (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
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
            .build_stream_client(
                "provider.compatible.stream",
                read_timeout_secs,
                5,
                &headers,
            )
    }

    fn chat_completions_url(&self) -> String {

        if let Some(ref api_path) = self.api_path {
            let separator = if api_path.starts_with('/') { "" } else { "/" };
            return format!("{}{separator}{api_path}", self.base_url);
        }

        let has_full_endpoint = reqwest::Url::parse(&self.base_url)
            .map(|url| {
                url.path()
                    .trim_end_matches('/')
                    .ends_with("/chat/completions")
            })
            .unwrap_or_else(|_| {
                self.base_url
                    .trim_end_matches('/')
                    .ends_with("/chat/completions")
            });

        if has_full_endpoint {
            self.base_url.clone()
        } else {
            format!("{}/chat/completions", self.base_url)
        }
    }

    fn path_ends_with(&self, suffix: &str) -> bool {
        if let Ok(url) = reqwest::Url::parse(&self.base_url) {
            return url.path().trim_end_matches('/').ends_with(suffix);
        }

        self.base_url.trim_end_matches('/').ends_with(suffix)
    }

    fn has_explicit_api_path(&self) -> bool {
        let Ok(url) = reqwest::Url::parse(&self.base_url) else {
            return false;
        };

        let path = url.path().trim_end_matches('/');
        !path.is_empty() && path != "/"
    }

    fn requires_tool_stream(&self) -> bool {
        let host_requires_tool_stream = reqwest::Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| host == "api.z.ai" || host.ends_with(".z.ai"));

        host_requires_tool_stream || matches!(self.name.as_str(), "zai" | "z.ai")
    }

    fn tool_stream_for_tools(&self, has_tools: bool) -> Option<bool> {
        if has_tools && self.requires_tool_stream() {
            Some(true)
        } else {
            None
        }
    }

    fn responses_url(&self) -> String {
        if self.path_ends_with("/responses") {
            return self.base_url.clone();
        }

        let normalized_base = self.base_url.trim_end_matches('/');

        if let Some(prefix) = normalized_base.strip_suffix("/chat/completions") {
            return format!("{prefix}/responses");
        }

        if self.has_explicit_api_path() {
            format!("{normalized_base}/responses")
        } else {
            format!("{normalized_base}/v1/responses")
        }
    }

    fn reasoning_effort_for_model(&self, model: &str) -> Option<String> {
        let id = model.rsplit('/').next().unwrap_or(model);
        let supports_reasoning_effort = id.starts_with("gpt-5") || id.contains("codex");
        supports_reasoning_effort
            .then(|| self.reasoning_effort.clone())
            .flatten()
    }

    fn model_supports_thinking_param(&self, model: &str) -> bool {
        let vendor = self.name.to_ascii_lowercase();
        let id = model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();
        id.starts_with("glm-")
            || id.starts_with("minimax")
            || id.contains("minimax-")
            || id.starts_with("kimi-thinking")
            || vendor.contains("zhipu")
            || vendor.contains("z.ai")
            || vendor.contains("zai")
            || vendor.contains("bigmodel")
            || vendor.contains("minimax")
    }

    fn thinking_param_for_model(&self, model: &str) -> Option<serde_json::Value> {
        if !self.model_supports_thinking_param(model) {
            return None;
        }
        if self.is_thinking_blacklisted(model) {
            return None;
        }
        Some(serde_json::json!({
            "type": "enabled",
        }))
    }

    fn thinking_blacklist_key(&self, model: &str) -> String {
        let workspace = crate::session::current_session_context()
            .map(|c| c.workspace_key)
            .unwrap_or_else(|| "__no_session__".to_string());
        format!(
            "{}::{}::{}",
            workspace,
            self.name.to_ascii_lowercase(),
            model.to_ascii_lowercase()
        )
    }

    fn is_thinking_blacklisted(&self, model: &str) -> bool {
        let key = self.thinking_blacklist_key(model);
        let store = thinking_blacklist_store();
        store
            .read()
            .map(|set| set.contains(&key))
            .unwrap_or(false)
    }

    fn blacklist_thinking(&self, model: &str) {
        let key = self.thinking_blacklist_key(model);
        let store = thinking_blacklist_store();
        if let Ok(mut set) = store.write() {
            set.insert(key);
        }
        persist_probe_cache();
    }

    fn is_thinking_param_unsupported(status: reqwest::StatusCode, error: &str) -> bool {
        if !matches!(
            status,
            reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY
        ) {
            return false;
        }
        let lower = error.to_lowercase();
        lower.contains("thinking") || lower.contains("reasoning")
    }

    fn reserved_output_tokens(&self, model: &str) -> usize {
        let window = self.context_window_for(model);
        let configured = self.max_tokens.map(|v| v as usize);
        let in_curator = matches!(
            crate::agent::coding_mode::active_coding_mode(),
            crate::agent::coding_mode::CodingMode::Curator
        );
        let (lo, hi) = if in_curator {
            (4096usize, 32768usize)
        } else {
            (512usize, 16384usize)
        };
        let default_reserve = (window / 8).clamp(lo, hi);
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
        id.starts_with("kimi-k2")
            || id.starts_with("moonshotai/kimi-k2")
            || id.starts_with("kimi-thinking")
    }

    fn model_supports_native_tools(model: &str) -> bool {
        let id = model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();

        let denylist_prefixes: [&str; 6] = [
            "moonshot-v1-8k",
            "moonshot-v1-32k",
            "moonshot-v1-128k",
            "moonshot-v1-auto",
            "moonshotai/moonshot-v1",
            "qwen-72b",
        ];
        if denylist_prefixes.iter().any(|p| id.starts_with(p)) {
            return false;
        }

        let denylist_substrings: [&str; 3] = [
            "-instruct-v0.1",
            "-instruct-v0.2",
            "-no-tools",
        ];
        if denylist_substrings.iter().any(|p| id.contains(p)) {
            return false;
        }

        true
    }
}

#[derive(Debug, Serialize)]
struct ApiChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptionsField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct StreamOptionsField {
    include_usage: bool,
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

impl UsageInfo {
    fn cached_input_tokens(&self) -> Option<u64> {
        self.prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens)
    }
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

const REASONING_PLACEHOLDER: &str =
    "(chain-of-thought unavailable for this turn  - placeholder injected to satisfy thinking-mode round-trip requirements)";

fn tool_spec_from_openai_json(value: &serde_json::Value) -> Option<crate::tools::ToolSpec> {
    let func = value.get("function").unwrap_or(value);
    let name = func.get("name").and_then(|v| v.as_str())?.to_string();
    if name.is_empty() {
        return None;
    }
    let description = func
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let parameters = func
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} }));
    Some(crate::tools::ToolSpec {
        name,
        description,
        parameters,
    })
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

#[derive(Debug, Deserialize, Serialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,

    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCall>>,
}

impl ResponseMessage {

    fn effective_content(&self) -> String {
        if let Some(content) = self.content.as_ref().filter(|c| !c.is_empty()) {
            let stripped = strip_think_tags(content);
            if !stripped.is_empty() {
                return stripped;
            }
        }

        self.reasoning_content
            .as_ref()
            .map(|c| strip_think_tags(c))
            .filter(|c| !c.is_empty())
            .unwrap_or_default()
    }

    fn effective_content_optional(&self) -> Option<String> {
        if let Some(content) = self.content.as_ref().filter(|c| !c.is_empty()) {
            let stripped = strip_think_tags(content);
            if !stripped.is_empty() {
                return Some(stripped);
            }
        }

        self.reasoning_content
            .as_ref()
            .map(|c| strip_think_tags(c))
            .filter(|c| !c.is_empty())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    function: Option<Function>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arguments: Option<String>,

    #[serde(
        rename = "parameters",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    parameters: Option<serde_json::Value>,
}

impl ToolCall {

    fn function_name(&self) -> Option<String> {

        if let Some(ref func) = self.function {
            if let Some(ref name) = func.name {
                return Some(name.clone());
            }
        }

        self.name.clone()
    }

    fn function_arguments(&self) -> Option<String> {

        if let Some(ref func) = self.function {
            if let Some(ref args) = func.arguments {
                return Some(args.clone());
            }
        }

        if let Some(ref args) = self.arguments {
            return Some(args.clone());
        }

        if let Some(ref params) = self.parameters {
            return serde_json::to_string(params).ok();
        }
        None
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Function {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

use crate::providers::sanitize::skip_serializing_tool_calls;

#[derive(Debug, Serialize)]
struct NativeChatRequest {
    model: String,
    messages: Vec<NativeMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptionsField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "skip_serializing_tool_calls")]
    tool_calls: Option<Vec<ToolCall>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<ResponsesInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ResponsesInput {
    role: String,
    content: ResponsesInputContent,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ResponsesInputContent {
    Text(String),
    Parts(Vec<ResponsesInputPart>),
}

#[derive(Debug, Serialize)]
struct ResponsesInputPart {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

impl ResponsesInput {
    fn user_text(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content: ResponsesInputContent::Text(content),
            kind: None,
        }
    }

    fn assistant_output_text(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            content: ResponsesInputContent::Parts(vec![ResponsesInputPart {
                kind: "output_text".to_string(),
                text: content,
            }]),
            kind: Some("message".to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output: Vec<ResponsesOutput>,
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutput {
    #[serde(default)]
    content: Vec<ResponsesContent>,
}

#[derive(Debug, Deserialize)]
struct ResponsesContent {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

use crate::providers::core::openai_sse::{sse_bytes_to_chunks, sse_bytes_to_events};

fn first_nonempty(text: Option<&str>) -> Option<String> {
    text.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn build_responses_prompt(messages: &[ChatMessage]) -> (Option<String>, Vec<ResponsesInput>) {
    let mut instructions_parts = Vec::new();
    let mut input = Vec::new();

    for message in messages {
        if message.content.trim().is_empty() {
            continue;
        }

        if message.role == "system" {
            instructions_parts.push(message.content.clone());
            continue;
        }

        let input_item = match message.role.as_str() {

            "assistant" | "tool" => ResponsesInput::assistant_output_text(message.content.clone()),
            _ => ResponsesInput::user_text(message.content.clone()),
        };
        input.push(input_item);
    }

    let instructions = if instructions_parts.is_empty() {
        None
    } else {
        Some(instructions_parts.join("\n\n"))
    };

    (instructions, input)
}

fn extract_responses_text(response: ResponsesResponse) -> Option<String> {
    if let Some(text) = first_nonempty(response.output_text.as_deref()) {
        return Some(text);
    }

    for item in &response.output {
        for content in &item.content {
            if content.kind.as_deref() == Some("output_text") {
                if let Some(text) = first_nonempty(content.text.as_deref()) {
                    return Some(text);
                }
            }
        }
    }

    for item in &response.output {
        for content in &item.content {
            if let Some(text) = first_nonempty(content.text.as_deref()) {
                return Some(text);
            }
        }
    }

    None
}

fn compact_sanitized_body_snippet(body: &str) -> String {
    super::sanitize_api_error(body)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_chat_response_body(provider_name: &str, body: &str) -> anyhow::Result<ApiChatResponse> {
    serde_json::from_str::<ApiChatResponse>(body).map_err(|error| {
        let snippet = compact_sanitized_body_snippet(body);
        anyhow::anyhow!(
            "{provider_name} API returned an unexpected chat-completions payload: {error}; body={snippet}"
        )
    })
}

fn parse_responses_response_body(
    provider_name: &str,
    body: &str,
) -> anyhow::Result<ResponsesResponse> {
    serde_json::from_str::<ResponsesResponse>(body).map_err(|error| {
        let snippet = compact_sanitized_body_snippet(body);
        anyhow::anyhow!(
            "{provider_name} Responses API returned an unexpected payload: {error}; body={snippet}"
        )
    })
}

impl OpenAiCompatibleProvider {
    fn apply_auth_header(
        &self,
        req: reqwest::RequestBuilder,
        credential: &str,
    ) -> reqwest::RequestBuilder {
        let req = crate::providers::core::idempotency::apply_idempotency_header(req);
        match &self.auth_header {
            AuthStyle::Bearer => req.header("Authorization", format!("Bearer {credential}")),
            AuthStyle::XApiKey => req.header("x-api-key", credential),
            AuthStyle::Custom(header) => req.header(header, credential),
        }
    }

    async fn chat_via_responses(
        &self,
        credential: &str,
        messages: &[ChatMessage],
        model: &str,
    ) -> anyhow::Result<String> {
        if self.responses_endpoint_marked_missing() {
            anyhow::bail!(RESPONSES_ENDPOINT_MISSING_MARKER);
        }
        let (instructions, input) = build_responses_prompt(messages);
        if input.is_empty() {
            anyhow::bail!(
                "{} Responses API fallback requires at least one non-system message",
                self.name
            );
        }

        let request = ResponsesRequest {
            model: model.to_string(),
            input,
            instructions,
            stream: Some(false),
        };

        let url = self.responses_url();

        let response = self
            .apply_auth_header(self.http_client().post(&url).json(&request), credential)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::NOT_FOUND
                || error.contains("url.not_found")
                || error.to_ascii_lowercase().contains("page not found")
            {
                self.mark_responses_endpoint_missing();
                tracing::info!(
                    target: "providers.compatible.responses",
                    provider = %self.name,
                    status = %status,
                    "Responses API endpoint not available on upstream; disabling fallback for this provider instance"
                );
                anyhow::bail!(RESPONSES_ENDPOINT_MISSING_MARKER);
            }
            anyhow::bail!("{} Responses API error: {error}", self.name);
        }

        let body = response.text().await?;
        let responses = parse_responses_response_body(&self.name, &body)?;
        if let Some(u) = responses.usage.as_ref() {
            crate::providers::record_text_path_usage(
                &self.name,
                model,
                u.input_tokens,
                u.output_tokens,
                None,
            );
        }

        extract_responses_text(responses)
            .ok_or_else(|| anyhow::anyhow!("No response from {} Responses API", self.name))
    }

    fn responses_endpoint_marker_key(&self) -> String {
        format!("{}::{}", self.name.to_ascii_lowercase(), self.base_url)
    }

    fn responses_endpoint_marked_missing(&self) -> bool {
        let store = responses_endpoint_missing_store();
        store
            .read()
            .map(|set| set.contains(&self.responses_endpoint_marker_key()))
            .unwrap_or(false)
    }

    fn mark_responses_endpoint_missing(&self) {
        let key = self.responses_endpoint_marker_key();
        let store = responses_endpoint_missing_store();
        if let Ok(mut set) = store.write() {
            set.insert(key);
        }
        persist_probe_cache();
    }

    fn convert_tool_specs(
        tools: Option<&[crate::tools::ToolSpec]>,
    ) -> Option<Vec<serde_json::Value>> {
        static CACHE: std::sync::LazyLock<crate::tools::spec_cache::ToolSpecCache> =
            std::sync::LazyLock::new(crate::tools::spec_cache::ToolSpecCache::new);
        tools.map(|items| {
            let serialized = CACHE.get_or_compute("openai-compatible", items, |specs| {
                let arr: Vec<serde_json::Value> = crate::tools::dedupe_tool_specs(specs)
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
                    .collect();
                serde_json::to_string(&arr).unwrap_or_default()
            });
            serde_json::from_str(&serialized).unwrap_or_default()
        })
    }

    fn to_message_content(
        role: &str,
        content: &str,
        allow_user_image_parts: bool,
    ) -> MessageContent {
        if role != "user" || !allow_user_image_parts {
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

    fn native_assistant_without_tool_calls_from_json_value(
        value: &serde_json::Value,
    ) -> NativeMessage {
        let reasoning_content = value
            .get("reasoning_content")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let content = match value.get("content") {
            Some(serde_json::Value::String(s)) if !s.is_empty() => {
                Some(MessageContent::Text(s.clone()))
            }
            _ => None,
        };
        NativeMessage {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content,
        }
    }

    fn looks_like_dispatcher_assistant_envelope(value: &serde_json::Value) -> bool {
        let Some(obj) = value.as_object() else {
            return false;
        };
        if obj.is_empty() {
            return false;
        }
        let mut has_envelope_key = false;
        for key in obj.keys() {
            match key.as_str() {
                "content" | "tool_calls" | "reasoning_content" => has_envelope_key = true,
                _ => return false,
            }
        }
        has_envelope_key
    }

    fn native_message_body_as_plain(content: &Option<MessageContent>) -> String {
        match content {
            None => String::new(),
            Some(MessageContent::Text(t)) => t.clone(),
            Some(MessageContent::Parts(parts)) => parts
                .iter()
                .filter_map(|p| match p {
                    MessagePart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    fn native_recovered_user_from_orphan_tool(m: NativeMessage) -> NativeMessage {
        let id = m.tool_call_id.unwrap_or_else(|| "unknown".to_string());
        let body = Self::native_message_body_as_plain(&m.content);
        let text = format!(
            "[Recovered tool output; assistant.tool_calls preamble was missing in transcript]\n\
             tool_call_id={id}\n\
             {body}",
        );
        NativeMessage {
            role: "user".to_string(),
            content: Some(MessageContent::Text(text)),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }
    }

    fn convert_messages_for_native(
        messages: &[ChatMessage],
        allow_user_image_parts: bool,
    ) -> Vec<NativeMessage> {
        Self::sanitize_native_tool_adjacency(
            messages
                .iter()
                .map(|message| {
                if message.role == "assistant"
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content)
                    && Self::looks_like_dispatcher_assistant_envelope(&value)
                {
                    let parsed_tool_calls = value
                        .get("tool_calls")
                        .and_then(|tc| {
                            serde_json::from_value::<Vec<ProviderToolCall>>(tc.clone()).ok()
                        })
                        .unwrap_or_default();

                    if !parsed_tool_calls.is_empty() {
                        let tool_calls = parsed_tool_calls
                            .into_iter()
                            .map(|tc| ToolCall {
                                id: Some(tc.id),
                                kind: Some("function".to_string()),
                                function: Some(Function {
                                    name: Some(tc.name),
                                    arguments: Some(tc.arguments),
                                }),
                                name: None,
                                arguments: None,
                                parameters: None,
                            })
                            .collect::<Vec<_>>();

                        let content = value
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(|value| MessageContent::Text(value.to_string()));

                        let reasoning_content = value
                            .get("reasoning_content")
                            .and_then(serde_json::Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(ToString::to_string)
                            .or_else(|| Some(REASONING_PLACEHOLDER.to_string()));

                        return NativeMessage {
                            role: "assistant".to_string(),
                            content,
                            tool_call_id: None,
                            tool_calls: Some(tool_calls),
                            reasoning_content,
                        };
                    }

                    let mut without_calls = value.clone();
                    if let Some(obj) = without_calls.as_object_mut() {
                        obj.remove("tool_calls");
                    }
                    return Self::native_assistant_without_tool_calls_from_json_value(
                        &without_calls,
                    );
                }

                if message.role == "tool" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content) {
                        let tool_call_id = value
                            .get("tool_call_id")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string);
                        let content = value
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .map(|value| MessageContent::Text(value.to_string()))
                            .or_else(|| Some(MessageContent::Text(message.content.clone())));

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
                    role: message.role.clone(),
                    content: Some(Self::to_message_content(
                        &message.role,
                        &message.content,
                        allow_user_image_parts,
                    )),
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                }
            })
            .collect(),
        )
    }

    fn sanitize_native_tool_adjacency(messages: Vec<NativeMessage>) -> Vec<NativeMessage> {
        let mut out: Vec<NativeMessage> = Vec::with_capacity(messages.len());
        for mut m in messages {
            if m.role == "assistant" {
                if m.tool_calls.as_ref().is_some_and(Vec::is_empty) {
                    m.tool_calls = None;
                }

                if m.role == "assistant"
                    && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
                    && m.reasoning_content
                        .as_ref()
                        .is_none_or(|s| s.is_empty())
                {
                    tracing::debug!(
                        target: "providers.compatible",
                        "injecting reasoning_content placeholder on tool-call assistant (round-trip safety)"
                    );
                    m.reasoning_content = Some(REASONING_PLACEHOLDER.to_string());
                }
            }

            if m.role == "assistant" && m.tool_calls.is_none() {
                let body_empty =
                    Self::native_message_body_as_plain(&m.content).trim().is_empty();
                if body_empty {
                    match m.reasoning_content.take().filter(|s| !s.trim().is_empty()) {
                        Some(reasoning) => {
                            m.content = Some(MessageContent::Text(reasoning));
                        }
                        None => {
                            tracing::warn!(
                                target: "providers.compatible",
                                "dropping empty assistant message (no content/tool_calls) to satisfy chat-completions validation"
                            );
                            continue;
                        }
                    }
                }
            }
            if m.role == "tool" {
                let preceded_ok = out.last().is_some_and(|last| {
                    if last.role == "tool" {
                        return true;
                    }
                    if last.role == "assistant" {
                        return last
                            .tool_calls
                            .as_ref()
                            .map(|calls| !calls.is_empty())
                            .unwrap_or(false);
                    }
                    false
                });
                if !preceded_ok {
                    tracing::warn!(
                        target: "providers.compatible",
                        "coercing orphan role=tool message into recovered user text (invalid pairing preamble)"
                    );
                    out.push(Self::native_recovered_user_from_orphan_tool(m));
                    continue;
                }
            }
            out.push(m);
        }
        Self::pad_missing_native_tool_followups(out)
    }

    fn pad_missing_native_tool_followups(mut msgs: Vec<NativeMessage>) -> Vec<NativeMessage> {
        const STUB: &str = "[Synthetic tool reply] No stored result for this tool_call_id in the wire \
                            batch (context trim or hydration). Ignore and continue.";
        let mut i = 0usize;
        while i < msgs.len() {
            if msgs[i].role != "assistant" {
                i += 1;
                continue;
            }
            let Some(ref calls) = msgs[i].tool_calls else {
                i += 1;
                continue;
            };
            if calls.is_empty() {
                i += 1;
                continue;
            }
            let required: Vec<String> = calls
                .iter()
                .filter_map(|c| {
                    c.id
                        .as_ref()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
                .collect();
            if required.is_empty() {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            let mut seen = std::collections::HashSet::<String>::new();
            while j < msgs.len() && msgs[j].role == "tool" {
                if let Some(id) = msgs[j].tool_call_id.as_ref() {
                    let t = id.trim();
                    if !t.is_empty() {
                        seen.insert(id.clone());
                    }
                }
                j += 1;
            }
            let missing: Vec<String> = required.into_iter().filter(|id| !seen.contains(id)).collect();
            if missing.is_empty() {
                i += 1;
                continue;
            }
            tracing::warn!(
                target: "providers.compatible",
                missing = ?missing,
                "inserting synthetic native tool messages after incomplete assistant.tool_calls"
            );
            for (offs, mid) in missing.into_iter().enumerate() {
                msgs.insert(
                    j + offs,
                    NativeMessage {
                        role: "tool".to_string(),
                        content: Some(MessageContent::Text(STUB.to_string())),
                        tool_call_id: Some(mid),
                        tool_calls: None,
                        reasoning_content: None,
                    },
                );
            }
            i += 1;
        }
        msgs
    }

    fn with_prompt_guided_tool_instructions(
        messages: &[ChatMessage],
        tools: Option<&[crate::tools::ToolSpec]>,
    ) -> Vec<ChatMessage> {
        let Some(tools) = tools else {
            return messages.to_vec();
        };

        if tools.is_empty() {
            return messages.to_vec();
        }

        let instructions = crate::providers::traits::build_tool_instructions_text(tools);
        let mut modified_messages = messages.to_vec();

        if let Some(system_message) = modified_messages.iter_mut().find(|m| m.role == "system") {
            if !system_message.content.is_empty() {
                system_message.content.push_str("\n\n");
            }
            system_message.content.push_str(&instructions);
        } else {
            modified_messages.insert(0, ChatMessage::system(instructions));
        }

        modified_messages
    }

    fn parse_native_response(message: ResponseMessage) -> ProviderChatResponse {
        let text = message.effective_content_optional();
        let reasoning_content = message.reasoning_content.clone();
        let tool_calls = message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                let name = tc.function_name()?;
                let arguments = tc.function_arguments().unwrap_or_else(|| "{}".to_string());
                let normalized_arguments =
                    crate::providers::sanitize::normalize_tool_call_arguments(&name, arguments);
                Some(ProviderToolCall {
                    id: tc.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    name,
                    arguments: normalized_arguments,
                })
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
            "this model does not support multimodal",
            "model does not support image",
        ]
        .iter()
        .any(|hint| lower.contains(hint))
    }

    fn structured_blacklist_key(&self, model: &str) -> String {
        format!("{}::{}::json_schema", self.name, model)
    }

    fn is_structured_output_blacklisted(&self, model: &str) -> bool {
        structured_output_blacklist()
            .read()
            .map(|set| set.contains(&self.structured_blacklist_key(model)))
            .unwrap_or(false)
    }

    fn blacklist_structured_output(&self, model: &str) {
        if let Ok(mut set) = structured_output_blacklist().write() {
            set.insert(self.structured_blacklist_key(model));
        }
    }

    fn is_response_format_unsupported(status: reqwest::StatusCode, error: &str) -> bool {
        if !matches!(
            status,
            reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY
        ) {
            return false;
        }
        let lower = error.to_ascii_lowercase();
        [
            "response_format",
            "json_schema",
            "json schema",
            "structured output",
            "structured_output",
        ]
        .iter()
        .any(|hint| lower.contains(hint))
    }

    async fn chat_structured_prompt_fallback(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<crate::providers::traits::StructuredResponse> {
        let mut augmented: Vec<ChatMessage> = messages.to_vec();
        let schema_text =
            serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string());
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
        let raw = self.chat_with_history(&augmented, model, temperature).await?;
        let parsed =
            crate::providers::traits::parse_first_json_object(&raw).ok_or_else(|| {
                anyhow::anyhow!(
                    "Provider returned no JSON object after schema-constrained chat: {raw}"
                )
            })?;
        Ok(crate::providers::traits::StructuredResponse {
            data: parsed,
            raw_text: raw,
            usage: None,
        })
    }
}

fn structured_output_blacklist() -> &'static std::sync::RwLock<std::collections::HashSet<String>>
{
    static STORE: std::sync::OnceLock<std::sync::RwLock<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    STORE.get_or_init(|| std::sync::RwLock::new(std::collections::HashSet::new()))
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn capabilities(&self) -> crate::providers::traits::ProviderCapabilities {
        crate::providers::traits::ProviderCapabilities {
            native_tool_calling: self.native_tool_calling,
            vision: self.supports_vision,
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
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "{} API key not set. Run `sen onboard` or set the appropriate env var.",
                self.name
            )
        })?;

        let mut messages = Vec::new();

        if self.merge_system_into_user {
            let content = match system_prompt {
                Some(sys) => format!("{sys}\n\n{message}"),
                None => message.to_string(),
            };
            messages.push(Message {
                role: "user".to_string(),
                content: Self::to_message_content("user", &content, !self.merge_system_into_user),
            });
        } else {
            if let Some(sys) = system_prompt {
                messages.push(Message {
                    role: "system".to_string(),
                    content: MessageContent::Text(sys.to_string()),
                });
            }
            messages.push(Message {
                role: "user".to_string(),
                content: Self::to_message_content("user", message, true),
            });
        }

        let request = ApiChatRequest {
            model: model.to_string(),
            messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            thinking: self.thinking_param_for_model(model),
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: self.max_tokens,
        };

        let url = self.chat_completions_url();

        let mut fallback_messages = Vec::new();
        if let Some(system_prompt) = system_prompt {
            fallback_messages.push(ChatMessage::system(system_prompt));
        }
        fallback_messages.push(ChatMessage::user(message));
        let fallback_messages = if self.merge_system_into_user {
            Self::flatten_system_messages(&fallback_messages)
        } else {
            fallback_messages
        };

        let response = match self
            .apply_auth_header(self.http_client().post(&url).json(&request), credential)
            .send()
            .await
        {
            Ok(response) => response,
            Err(chat_error) => {
                if self.supports_responses_fallback && !self.responses_endpoint_marked_missing() {
                    let sanitized = super::sanitize_api_error(&chat_error.to_string());
                    let provider_name = self.name.clone();
                    return self
                        .chat_via_responses(credential, &fallback_messages, model)
                        .await
                        .map_err(|responses_err| {
                            if is_responses_endpoint_missing_error(&responses_err) {
                                anyhow::anyhow!(
                                    "{provider_name} chat completions transport error: {sanitized}"
                                )
                            } else {
                                anyhow::anyhow!(
                                    "{provider_name} chat completions transport error: {sanitized} (responses fallback failed: {responses_err})"
                                )
                            }
                        });
                }

                return Err(chat_error.into());
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await?;
            let sanitized = super::sanitize_api_error(&error);

            if !self.is_thinking_blacklisted(model)
                && Self::is_thinking_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.compatible",
                    provider = %self.name,
                    model,
                    status = %status,
                    "thinking parameter rejected by upstream ({sanitized}); blacklisting model and retrying without thinking"
                );
                self.blacklist_thinking(model);
                return Box::pin(self.chat_with_system(
                    system_prompt,
                    message,
                    model,
                    temperature,
                ))
                .await;
            }

            if status == reqwest::StatusCode::NOT_FOUND
                && self.supports_responses_fallback
                && !self.responses_endpoint_marked_missing()
            {
                let provider_name = self.name.clone();
                return self
                    .chat_via_responses(credential, &fallback_messages, model)
                    .await
                    .map_err(|responses_err| {
                        if is_responses_endpoint_missing_error(&responses_err) {
                            anyhow::anyhow!(
                                "{provider_name} API error ({status}): {sanitized}"
                            )
                        } else {
                            anyhow::anyhow!(
                                "{provider_name} API error ({status}): {sanitized} (chat completions unavailable; responses fallback failed: {responses_err})"
                            )
                        }
                    });
            }

            anyhow::bail!("{} API error ({status}): {sanitized}", self.name);
        }

        let body = response.text().await?;
        let chat_response = parse_chat_response_body(&self.name, &body)?;
        if let Some(u) = chat_response.usage.as_ref() {
            crate::providers::record_text_path_usage(
                &self.name,
                model,
                u.prompt_tokens,
                u.completion_tokens,
                u.prompt_tokens_details
                    .as_ref()
                    .and_then(|d| d.cached_tokens),
            );
        }

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| {

                if c.message.tool_calls.is_some()
                    && c.message
                        .tool_calls
                        .as_ref()
                        .map_or(false, |t| !t.is_empty())
                {
                    serde_json::to_string(&c.message)
                        .unwrap_or_else(|_| c.message.effective_content())
                } else {

                    c.message.effective_content()
                }
            })
            .ok_or_else(|| anyhow::anyhow!("No response from {}", self.name))
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "{} API key not set. Run `sen onboard` or set the appropriate env var.",
                self.name
            )
        })?;

        let sanitized_input = super::traits::flatten_messages_for_text_only_wire(messages);
        let budgeted_input = super::traits::enforce_context_budget_with_window(
            sanitized_input,
            model,
            self.reserved_output_tokens(model),
            Some(self.context_window_for(model)),
        );
        let effective_messages = if self.merge_system_into_user {
            Self::flatten_system_messages(&budgeted_input)
        } else {
            Self::normalize_system_for_strict_wire(&budgeted_input)
        };
        let api_messages: Vec<Message> = effective_messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(
                    &m.role,
                    &m.content,
                    !self.merge_system_into_user,
                ),
            })
            .collect();

        let request = ApiChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            thinking: self.thinking_param_for_model(model),
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: self.max_tokens,
        };

        let url = self.chat_completions_url();
        let response = match self
            .apply_auth_header(self.http_client().post(&url).json(&request), credential)
            .send()
            .await
        {
            Ok(response) => response,
            Err(chat_error) => {
                if self.supports_responses_fallback && !self.responses_endpoint_marked_missing() {
                    let sanitized = super::sanitize_api_error(&chat_error.to_string());
                    let provider_name = self.name.clone();
                    return self
                        .chat_via_responses(credential, &effective_messages, model)
                        .await
                        .map_err(|responses_err| {
                            if is_responses_endpoint_missing_error(&responses_err) {
                                anyhow::anyhow!(
                                    "{provider_name} chat completions transport error: {sanitized}"
                                )
                            } else {
                                anyhow::anyhow!(
                                    "{provider_name} chat completions transport error: {sanitized} (responses fallback failed: {responses_err})"
                                )
                            }
                        });
                }

                return Err(chat_error.into());
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            let sanitized = super::sanitize_api_error(&error_body);

            if !self.is_thinking_blacklisted(model)
                && Self::is_thinking_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.compatible",
                    provider = %self.name,
                    model,
                    status = %status,
                    "thinking parameter rejected by upstream ({sanitized}); blacklisting model and retrying without thinking"
                );
                self.blacklist_thinking(model);
                return Box::pin(self.chat_with_history(messages, model, temperature)).await;
            }

            if status == reqwest::StatusCode::NOT_FOUND
                && self.supports_responses_fallback
                && !self.responses_endpoint_marked_missing()
            {
                let provider_name = self.name.clone();
                return self
                    .chat_via_responses(credential, &effective_messages, model)
                    .await
                    .map_err(|responses_err| {
                        if is_responses_endpoint_missing_error(&responses_err) {
                            anyhow::anyhow!(
                                "{provider_name} API error ({status}): {sanitized}"
                            )
                        } else {
                            anyhow::anyhow!(
                                "{provider_name} API error ({status}): {sanitized} (chat completions unavailable; responses fallback failed: {responses_err})"
                            )
                        }
                    });
            }

            anyhow::bail!("{} API error ({status}): {sanitized}", self.name);
        }

        let body = response.text().await?;
        let chat_response = parse_chat_response_body(&self.name, &body)?;
        if let Some(u) = chat_response.usage.as_ref() {
            crate::providers::record_text_path_usage(
                &self.name,
                model,
                u.prompt_tokens,
                u.completion_tokens,
                u.prompt_tokens_details
                    .as_ref()
                    .and_then(|d| d.cached_tokens),
            );
        }

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| {

                if c.message.tool_calls.is_some()
                    && c.message
                        .tool_calls
                        .as_ref()
                        .map_or(false, |t| !t.is_empty())
                {
                    serde_json::to_string(&c.message)
                        .unwrap_or_else(|_| c.message.effective_content())
                } else {

                    c.message.effective_content()
                }
            })
            .ok_or_else(|| anyhow::anyhow!("No response from {}", self.name))
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
                "{} API key not set. Run `sen onboard` or set the appropriate env var.",
                self.name
            )
        })?;

        let pre_budget = if self.merge_system_into_user {
            Self::flatten_system_messages(messages)
        } else {
            Self::normalize_system_for_strict_wire(messages)
        };
        let json_tools_reserve: usize = tools
            .iter()
            .map(|t| t.to_string().len().div_ceil(4))
            .sum();
        let effective_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            pre_budget,
            model,
            self.reserved_output_tokens(model)
                .saturating_add(json_tools_reserve),
            Some(self.context_window_for(model)),
        );
        let api_messages: Vec<Message> = effective_messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(
                    &m.role,
                    &m.content,
                    !self.merge_system_into_user,
                ),
            })
            .collect();

        let model_supports_native = Self::model_supports_native_tools(model);
        let has_tools = !tools.is_empty();
        let allow_native_tools = has_tools && model_supports_native;

        if !model_supports_native {
            tracing::debug!(
                target: "providers.compatible",
                provider = %self.name,
                model,
                has_tools,
                "model is on the legacy/no-tools allowlist; routing through chat_with_history with prompt-guided tools"
            );
            let guided = if has_tools {
                let specs: Vec<crate::tools::ToolSpec> = tools
                    .iter()
                    .filter_map(tool_spec_from_openai_json)
                    .collect();
                if specs.is_empty() {
                    messages.to_vec()
                } else {
                    Self::with_prompt_guided_tool_instructions(messages, Some(&specs))
                }
            } else {
                messages.to_vec()
            };
            let text = self.chat_with_history(&guided, model, temperature).await?;
            return Ok(ProviderChatResponse::text_only(Some(text), None));
        }

        let request = ApiChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            thinking: self.thinking_param_for_model(model),
            tool_stream: self.tool_stream_for_tools(allow_native_tools),
            tools: if allow_native_tools {
                Some(tools.to_vec())
            } else {
                None
            },
            tool_choice: if allow_native_tools {
                Some("auto".to_string())
            } else {
                None
            },
            max_tokens: self.max_tokens,
        };

        let url = self.chat_completions_url();
        let response = match self
            .apply_auth_header(self.http_client().post(&url).json(&request), credential)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(
                    "{} native tool call transport failed: {error}; falling back to history path",
                    self.name
                );
                let text = self.chat_with_history(messages, model, temperature).await?;
                return Ok(ProviderChatResponse::text_only(Some(text), None));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            let sanitized = super::sanitize_api_error(&error_body);

            if !self.is_thinking_blacklisted(model)
                && Self::is_thinking_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.compatible",
                    provider = %self.name,
                    model,
                    status = %status,
                    "thinking parameter rejected by upstream ({sanitized}); blacklisting model and retrying without thinking"
                );
                self.blacklist_thinking(model);
                return Box::pin(self.chat_with_tools(messages, tools, model, temperature))
                    .await;
            }

            if Self::is_native_tool_schema_unsupported(status, &sanitized) {
                tracing::warn!(
                    target: "providers.compatible",
                    provider = %self.name,
                    model,
                    status = %status,
                    "native tools rejected by upstream ({sanitized}); retrying via chat_with_history"
                );
                let text = self.chat_with_history(messages, model, temperature).await?;
                return Ok(ProviderChatResponse::text_only(Some(text), None));
            }

            anyhow::bail!("{} API error ({status}): {sanitized}", self.name);
        }

        let body = response.text().await?;
        let chat_response = parse_chat_response_body(&self.name, &body)?;
        let usage = chat_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: u.cached_input_tokens(),
            cache_creation_input_tokens: None,
        });
        let choice = chat_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from {}", self.name))?;

        let stop_reason = choice
            .finish_reason
            .as_deref()
            .and_then(crate::providers::traits::StopReason::from_wire);
        let mut result = Self::parse_native_response(choice.message);
        result.usage = usage;
        result.stop_reason = stop_reason;
        Ok(result)
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "{} API key not set. Run `sen onboard` or set the appropriate env var.",
                self.name
            )
        })?;

        let model_supports_native = Self::model_supports_native_tools(model);
        let has_tools = request.tools.is_some_and(|t| !t.is_empty());
        let allow_native_tools = has_tools && model_supports_native;

        if !model_supports_native {
            tracing::debug!(
                target: "providers.compatible",
                provider = %self.name,
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
            Self::convert_tool_specs(request.tools)
        } else {
            None
        };
        let tools_reserve = if allow_native_tools {
            request
                .tools
                .map(crate::providers::traits::estimate_tool_specs_tokens)
                .unwrap_or(0)
        } else {
            0
        };
        let pre_budget = if self.merge_system_into_user {
            Self::flatten_system_messages(request.messages)
        } else {
            Self::normalize_system_for_strict_wire(request.messages)
        };
        let effective_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            pre_budget,
            model,
            self.reserved_output_tokens(model).saturating_add(tools_reserve),
            Some(self.context_window_for(model)),
        );
        let mut native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages_for_native(
                &effective_messages,
                !self.merge_system_into_user,
            ),
            temperature: Self::adjust_temperature_for_model(model, temperature),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            thinking: self.thinking_param_for_model(model),
            tool_stream: self
                .tool_stream_for_tools(tools.as_ref().is_some_and(|tools| !tools.is_empty())),
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: self.max_tokens,
        };
        if native_request.thinking.is_none() {
            for message in native_request.messages.iter_mut() {
                message.reasoning_content = None;
            }
        }

        let url = self.chat_completions_url();
        let response = match self
            .apply_auth_header(
                self.http_client().post(&url).json(&native_request),
                credential,
            )
            .send()
            .await
        {
            Ok(response) => response,
            Err(chat_error) => {
                let request_has_tools = request.tools.map(|t| !t.is_empty()).unwrap_or(false);
                if self.supports_responses_fallback
                    && !self.responses_endpoint_marked_missing()
                    && !request_has_tools
                {
                    let sanitized = super::sanitize_api_error(&chat_error.to_string());
                    let provider_name = self.name.clone();
                    return self
                        .chat_via_responses(credential, &effective_messages, model)
                        .await
                        .map(|text| ProviderChatResponse::text_only(Some(text), None))
                        .map_err(|responses_err| {
                            if is_responses_endpoint_missing_error(&responses_err) {
                                anyhow::anyhow!(
                                    "{provider_name} native chat transport error: {sanitized}"
                                )
                            } else {
                                anyhow::anyhow!(
                                    "{provider_name} native chat transport error: {sanitized} (responses fallback failed: {responses_err})"
                                )
                            }
                        });
                }

                return Err(chat_error.into());
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await?;
            let sanitized = super::sanitize_api_error(&error);

            if !self.is_thinking_blacklisted(model)
                && Self::is_thinking_param_unsupported(status, &sanitized)
            {
                tracing::warn!(
                    target: "providers.compatible",
                    provider = %self.name,
                    model,
                    status = %status,
                    "thinking parameter rejected by upstream ({sanitized}); blacklisting model and retrying without thinking"
                );
                self.blacklist_thinking(model);
                return Box::pin(self.chat(request, model, temperature)).await;
            }

            if Self::is_native_tool_schema_unsupported(status, &sanitized) {
                let fallback_messages =
                    Self::with_prompt_guided_tool_instructions(request.messages, request.tools);
                let text = self
                    .chat_with_history(&fallback_messages, model, temperature)
                    .await?;
                return Ok(ProviderChatResponse::text_only(Some(text), None));
            }

            let request_has_tools = request.tools.map(|t| !t.is_empty()).unwrap_or(false);
            if status == reqwest::StatusCode::NOT_FOUND
                && self.supports_responses_fallback
                && !self.responses_endpoint_marked_missing()
                && !request_has_tools
            {
                let provider_name = self.name.clone();
                return self
                    .chat_via_responses(credential, &effective_messages, model)
                    .await
                    .map(|text| ProviderChatResponse::text_only(Some(text), None))
                    .map_err(|responses_err| {
                        if is_responses_endpoint_missing_error(&responses_err) {
                            anyhow::anyhow!(
                                "{provider_name} API error ({status}): {sanitized}"
                            )
                        } else {
                            anyhow::anyhow!(
                                "{provider_name} API error ({status}): {sanitized} (chat completions unavailable; responses fallback failed: {responses_err})"
                            )
                        }
                    });
            }

            anyhow::bail!("{} API error ({status}): {sanitized}", self.name);
        }

        let native_response: ApiChatResponse = response.json().await?;
        let usage = native_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: u.cached_input_tokens(),
            cache_creation_input_tokens: None,
        });
        let choice = native_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from {}", self.name))?;

        let stop_reason = choice
            .finish_reason
            .as_deref()
            .and_then(crate::providers::traits::StopReason::from_wire);
        let mut result = Self::parse_native_response(choice.message);
        result.usage = usage;
        result.stop_reason = stop_reason;
        Ok(result)
    }

    fn supports_native_tools(&self) -> bool {
        self.native_tool_calling
    }

    fn consumes_reasoning_envelope(&self) -> bool {
        true
    }

    async fn chat_structured(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<crate::providers::traits::StructuredResponse> {
        if self.is_structured_output_blacklisted(model) {
            return self
                .chat_structured_prompt_fallback(messages, schema, model, temperature)
                .await;
        }
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "{} API key not set. Run `sen onboard` or set the appropriate env var.",
                self.name
            )
        })?;

        let sanitized = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            model,
            self.reserved_output_tokens(model),
            None,
        );
        let api_messages =
            Self::convert_messages_for_native(&sanitized, self.supports_vision);
        let body = serde_json::json!({
            "model": model,
            "messages": api_messages,
            "temperature": Self::adjust_temperature_for_model(model, temperature),
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_output",
                    "schema": crate::tools::schema::SchemaCleanr::prepare_for_strict_output(
                        schema.clone(),
                    ),
                    "strict": true
                }
            },
        });

        let url = self.chat_completions_url();
        let response = match self
            .apply_auth_header(self.http_client().post(&url).json(&body), credential)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(
                    target: "providers.compatible.structured",
                    provider = %self.name,
                    error = %e,
                    "structured chat transport error; falling back to prompt-injected schema"
                );
                return self
                    .chat_structured_prompt_fallback(messages, schema, model, temperature)
                    .await;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            let sanitized_err = super::sanitize_api_error(&error_body);
            if Self::is_response_format_unsupported(status, &sanitized_err) {
                tracing::info!(
                    target: "providers.compatible.structured",
                    provider = %self.name,
                    model,
                    status = %status,
                    "vendor rejected response_format json_schema; blacklisting and using prompt-injected fallback"
                );
                self.blacklist_structured_output(model);
                return self
                    .chat_structured_prompt_fallback(messages, schema, model, temperature)
                    .await;
            }
            anyhow::bail!("{} structured chat error ({status}): {sanitized_err}", self.name);
        }

        let native: ApiChatResponse = response.json().await?;
        let usage = native.usage.map(|u| crate::providers::traits::TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: u.cached_input_tokens(),
            cache_creation_input_tokens: None,
        });
        let raw = native
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.effective_content())
            .unwrap_or_default();
        let parsed = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .or_else(|| crate::providers::traits::parse_first_json_object(&raw))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} structured chat returned non-JSON payload: {raw}",
                    self.name
                )
            })?;
        Ok(crate::providers::traits::StructuredResponse {
            data: parsed,
            raw_text: raw,
            usage,
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_streaming_tool_events(&self) -> bool {
        self.native_tool_calling
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
                let provider_name = self.name.clone();
                return stream::once(async move {
                    Err(StreamError::Provider(format!(
                        "{} API key not set",
                        provider_name
                    )))
                })
                .boxed();
            }
        };

        let raw_messages_owned: std::sync::Arc<Vec<ChatMessage>> =
            std::sync::Arc::new(request.messages.to_vec());
        let raw_tools_owned: Option<Vec<crate::tools::ToolSpec>> =
            request.tools.map(|t| t.to_vec());

        let model_supports_native = Self::model_supports_native_tools(model);
        let has_tools = raw_tools_owned.as_ref().is_some_and(|t| !t.is_empty());
        let allow_native_tools = has_tools && model_supports_native;

        let mut effective_messages: Vec<ChatMessage> = if self.merge_system_into_user {
            Self::flatten_system_messages(&raw_messages_owned)
        } else {
            Self::normalize_system_for_strict_wire(&raw_messages_owned)
        };

        if !model_supports_native && has_tools {
            effective_messages = Self::with_prompt_guided_tool_instructions(
                &effective_messages,
                raw_tools_owned.as_deref(),
            );
        }

        if !allow_native_tools {
            effective_messages = super::traits::flatten_messages_for_text_only_wire(&effective_messages);
            effective_messages = super::traits::enforce_context_budget_with_window(
                effective_messages,
                model,
                self.reserved_output_tokens(model),
                Some(self.context_window_for(model)),
            );
        } else {
            let tools_reserve = raw_tools_owned
                .as_deref()
                .map(crate::providers::traits::estimate_tool_specs_tokens)
                .unwrap_or(0);
            effective_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
                self,
                effective_messages,
                model,
                self.reserved_output_tokens(model).saturating_add(tools_reserve),
                Some(self.context_window_for(model)),
            );
        }

        let tools = if allow_native_tools {
            Self::convert_tool_specs(raw_tools_owned.as_deref())
        } else {
            None
        };

        let use_native_wire = self.native_tool_calling && allow_native_tools;
        let payload = if use_native_wire {
            let tool_list_non_empty = tools.as_ref().is_some_and(|specs| !specs.is_empty());
            let mut native_messages = Self::convert_messages_for_native(
                &effective_messages,
                !self.merge_system_into_user,
            );
            let thinking_value = self.thinking_param_for_model(model);
            if thinking_value.is_none() {
                for message in native_messages.iter_mut() {
                    message.reasoning_content = None;
                }
            }
            serde_json::to_value(NativeChatRequest {
                model: model.to_string(),
                messages: native_messages,
                temperature: Self::adjust_temperature_for_model(model, temperature),
                reasoning_effort: self.reasoning_effort_for_model(model),
                thinking: thinking_value,
                tool_stream: if options.enabled {
                    self.tool_stream_for_tools(tool_list_non_empty)
                } else {
                    None
                },
                stream: Some(options.enabled),
                stream_options: if options.enabled {
                    Some(StreamOptionsField { include_usage: true })
                } else {
                    None
                },
                tools: tools.clone(),
                tool_choice: tools.as_ref().map(|_| "auto".to_string()),
                max_tokens: self.max_tokens,
            })
        } else {
            let messages = effective_messages
                .iter()
                .map(|message| Message {
                    role: message.role.clone(),
                    content: Self::to_message_content(
                        &message.role,
                        &message.content,
                        !self.merge_system_into_user,
                    ),
                })
                .collect();

            serde_json::to_value(ApiChatRequest {
                model: model.to_string(),
                messages,
                temperature: Self::adjust_temperature_for_model(model, temperature),
                reasoning_effort: self.reasoning_effort_for_model(model),
                thinking: self.thinking_param_for_model(model),
                tool_stream: None,
                stream: Some(options.enabled),
                stream_options: if options.enabled {
                    Some(StreamOptionsField { include_usage: true })
                } else {
                    None
                },
                tools: None,
                tool_choice: None,
                max_tokens: self.max_tokens,
            })
        };

        let payload = match payload {
            Ok(payload) => payload,
            Err(error) => {
                return stream::once(async move { Err(StreamError::Json(error)) }).boxed();
            }
        };

        let url = self.chat_completions_url();
        let client = self.stream_http_client();
        let auth_header = self.auth_header.clone();
        let count_tokens = options.count_tokens;
        let provider_clone = self.clone();
        let model_owned = model.to_string();
        let effective_arc: std::sync::Arc<Vec<ChatMessage>> = std::sync::Arc::new(effective_messages);
        let fallback_messages = std::sync::Arc::clone(&effective_arc);
        let fallback_tools = raw_tools_owned.clone();
        let fallback_temperature = temperature;
        let thinking_retry_messages = std::sync::Arc::clone(&raw_messages_owned);
        let thinking_retry_tools = raw_tools_owned.clone();
        let thinking_retry_options = options;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

        let _ = crate::runtime::spawn_supervised(
            "providers.compatible.chat_with_history_stream",
            async move {
                let mut req_builder = client.post(&url).json(&payload);

                req_builder = match &auth_header {
                    AuthStyle::Bearer => {
                        req_builder.header("Authorization", format!("Bearer {}", credential))
                    }
                    AuthStyle::XApiKey => req_builder.header("x-api-key", &credential),
                    AuthStyle::Custom(header) => req_builder.header(header, &credential),
                };
                req_builder = req_builder.header("Accept", "text/event-stream");

                let response = match req_builder.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                };

                if !response.status().is_success() {
                    let (status, error_body) =
                        super::stream_error_body_with_retry_after(response).await;
                    let sanitized = super::sanitize_api_error(&error_body);

                    if !provider_clone.is_thinking_blacklisted(&model_owned)
                        && Self::is_thinking_param_unsupported(status, &sanitized)
                    {
                        tracing::warn!(
                            target: "providers.compatible.stream",
                            provider = %provider_clone.name,
                            model = %model_owned,
                            status = %status,
                            "thinking parameter rejected by upstream ({sanitized}); blacklisting model and re-issuing stream without thinking"
                        );
                        provider_clone.blacklist_thinking(&model_owned);
                        let retry_request = crate::providers::traits::ChatRequest {
                            messages: thinking_retry_messages.as_slice(),
                            tools: thinking_retry_tools.as_deref(),
                        };
                        let mut retry_stream = provider_clone.stream_chat(
                            retry_request,
                            &model_owned,
                            fallback_temperature,
                            thinking_retry_options,
                        );
                        while let Some(event) = retry_stream.next().await {
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                        return;
                    }

                    if Self::is_native_tool_schema_unsupported(status, &sanitized) {
                        tracing::warn!(
                            target: "providers.compatible.stream",
                            provider = %provider_clone.name,
                            model = %model_owned,
                            status = %status,
                            "stream rejected by upstream ({sanitized}); falling back to non-streaming chat_with_history"
                        );
                        let guided = Self::with_prompt_guided_tool_instructions(
                            &fallback_messages,
                            fallback_tools.as_deref(),
                        );
                        match provider_clone
                            .chat_with_history(
                                &guided,
                                &model_owned,
                                fallback_temperature,
                            )
                            .await
                        {
                            Ok(text) => {
                                if !text.is_empty() {
                                    let chunk = StreamChunk {
                                        delta: text,
                                        is_final: false,
                                        token_count: 0,
                                        reasoning: None,
                                    };
                                    let _ = tx
                                        .send(Ok(StreamEvent::TextDelta(chunk)))
                                        .await;
                                }
                                let _ = tx.send(Ok(StreamEvent::Final)).await;
                            }
                            Err(fallback_err) => {
                                let _ = tx
                                    .send(Err(StreamError::Provider(format!(
                                        "{}: {} (fallback chat_with_history failed: {fallback_err})",
                                        status, sanitized
                                    ))))
                                    .await;
                            }
                        }
                        return;
                    }

                    let _ = tx
                        .send(Err(StreamError::Provider(format!(
                            "{}: {}",
                            status, sanitized
                        ))))
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
                let provider_name = self.name.clone();
                return stream::once(async move {
                    Err(StreamError::Provider(format!(
                        "{} API key not set",
                        provider_name
                    )))
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
            content: Self::to_message_content("user", message, !self.merge_system_into_user),
        });

        let request = ApiChatRequest {
            model: model.to_string(),
            messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            stream: Some(options.enabled),
            stream_options: if options.enabled {
                Some(StreamOptionsField { include_usage: true })
            } else {
                None
            },
            reasoning_effort: self.reasoning_effort_for_model(model),
            thinking: self.thinking_param_for_model(model),
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: self.max_tokens,
        };

        let url = self.chat_completions_url();
        let client = self.stream_http_client();
        let auth_header = self.auth_header.clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let provider_clone = self.clone();
        let model_owned = model.to_string();
        let system_prompt_owned = system_prompt.map(ToString::to_string);
        let message_owned = message.to_string();
        let temperature_owned = temperature;
        let options_owned = options;

        let _ = crate::runtime::spawn_supervised("providers.compatible.chat_stream", async move {

            let mut req_builder = client.post(&url).json(&request);

            req_builder = match &auth_header {
                AuthStyle::Bearer => {
                    req_builder.header("Authorization", format!("Bearer {}", credential))
                }
                AuthStyle::XApiKey => req_builder.header("x-api-key", &credential),
                AuthStyle::Custom(header) => req_builder.header(header, &credential),
            };

            req_builder = req_builder.header("Accept", "text/event-stream");

            let response = match req_builder.send().await {
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

                if !provider_clone.is_thinking_blacklisted(&model_owned)
                    && Self::is_thinking_param_unsupported(status, &sanitized)
                {
                    tracing::warn!(
                        target: "providers.compatible.stream",
                        provider = %provider_clone.name,
                        model = %model_owned,
                        status = %status,
                        "thinking parameter rejected by upstream ({sanitized}); blacklisting model and re-issuing stream without thinking"
                    );
                    provider_clone.blacklist_thinking(&model_owned);
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
                    .send(Err(StreamError::Provider(format!("{}: {}", status, error))))
                    .await;
                return;
            }

            let mut chunk_stream = sse_bytes_to_chunks(response, options.count_tokens);
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
                let provider_name = self.name.clone();
                return stream::once(async move {
                    Err(StreamError::Provider(format!(
                        "{} API key not set",
                        provider_name
                    )))
                })
                .boxed();
            }
        };

        let sanitized_input = super::traits::flatten_messages_for_text_only_wire(messages);
        let budgeted_input = super::traits::enforce_context_budget_with_window(
            sanitized_input,
            model,
            self.reserved_output_tokens(model),
            Some(self.context_window_for(model)),
        );
        let effective_messages = if self.merge_system_into_user {
            Self::flatten_system_messages(&budgeted_input)
        } else {
            Self::normalize_system_for_strict_wire(&budgeted_input)
        };
        let api_messages: Vec<Message> = effective_messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(
                    &m.role,
                    &m.content,
                    !self.merge_system_into_user,
                ),
            })
            .collect();

        let request = ApiChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature: Self::adjust_temperature_for_model(model, temperature),
            stream: Some(options.enabled),
            stream_options: if options.enabled {
                Some(StreamOptionsField { include_usage: true })
            } else {
                None
            },
            reasoning_effort: self.reasoning_effort_for_model(model),
            thinking: self.thinking_param_for_model(model),
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: self.max_tokens,
        };

        let url = self.chat_completions_url();
        let client = self.stream_http_client();
        let auth_header = self.auth_header.clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let provider_clone = self.clone();
        let model_owned = model.to_string();
        let temperature_owned = temperature;
        let options_owned = options;
        let retry_messages: Vec<ChatMessage> = messages.to_vec();

        let _ = crate::runtime::spawn_supervised(
            "providers.compatible.stream_chat_with_history",
            async move {
                let mut req_builder = client.post(&url).json(&request);

                req_builder = match &auth_header {
                    AuthStyle::Bearer => {
                        req_builder.header("Authorization", format!("Bearer {}", credential))
                    }
                    AuthStyle::XApiKey => req_builder.header("x-api-key", &credential),
                    AuthStyle::Custom(header) => req_builder.header(header, &credential),
                };

                req_builder = req_builder.header("Accept", "text/event-stream");

                let response = match req_builder.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let error = match response.text().await {
                        Ok(e) => e,
                        Err(_) => format!("HTTP error: {}", status),
                    };
                    let sanitized = super::sanitize_api_error(&error);

                    if !provider_clone.is_thinking_blacklisted(&model_owned)
                        && Self::is_thinking_param_unsupported(status, &sanitized)
                    {
                        tracing::warn!(
                            target: "providers.compatible.stream",
                            provider = %provider_clone.name,
                            model = %model_owned,
                            status = %status,
                            "thinking parameter rejected by upstream ({sanitized}); blacklisting model and re-issuing stream without thinking"
                        );
                        provider_clone.blacklist_thinking(&model_owned);
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
                        .send(Err(StreamError::Provider(format!("{}: {}", status, error))))
                        .await;
                    return;
                }

                let mut chunk_stream = sse_bytes_to_chunks(response, options.count_tokens);
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

    async fn warmup(&self) -> anyhow::Result<()> {
        if let Some(credential) = self.credential.as_ref() {

            let url = self.chat_completions_url();
            let _ = self
                .apply_auth_header(self.http_client().get(&url), credential)
                .send()
                .await?;
        }
        Ok(())
    }
}

fn provider_probe_cache_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|h| !h.is_empty()))?;
    Some(
        std::path::PathBuf::from(home)
            .join(".senweavercoding")
            .join("provider_probe_cache.json"),
    )
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ProviderProbeCache {
    #[serde(default)]
    thinking_blacklist: Vec<String>,
    #[serde(default)]
    responses_endpoint_missing: Vec<String>,
}

fn load_probe_cache() -> ProviderProbeCache {
    let Some(path) = provider_probe_cache_path() else {
        return ProviderProbeCache::default();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

pub(crate) fn persist_probe_cache() {
    let Some(path) = provider_probe_cache_path() else {
        return;
    };
    let cache = ProviderProbeCache {
        thinking_blacklist: thinking_blacklist_store()
            .read()
            .map(|s| {
                let mut v: Vec<String> = s.iter().cloned().collect();
                v.sort();
                v
            })
            .unwrap_or_default(),
        responses_endpoint_missing: responses_endpoint_missing_store()
            .read()
            .map(|s| {
                let mut v: Vec<String> = s.iter().cloned().collect();
                v.sort();
                v
            })
            .unwrap_or_default(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&cache) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = crate::util::atomic_write(&path, json.as_bytes()) {
            tracing::debug!(error = %e, "failed to persist provider probe cache");
        }
    }
}

fn thinking_blacklist_store()
-> &'static std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>> {
    static STORE: std::sync::OnceLock<
        std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    > = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        let seeded: std::collections::HashSet<String> =
            load_probe_cache().thinking_blacklist.into_iter().collect();
        std::sync::Arc::new(std::sync::RwLock::new(seeded))
    })
}

pub(crate) const RESPONSES_ENDPOINT_MISSING_MARKER: &str = "responses_endpoint_missing";

pub(crate) fn is_responses_endpoint_missing_error(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains(RESPONSES_ENDPOINT_MISSING_MARKER)
}

fn responses_endpoint_missing_store()
-> &'static std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>> {
    static STORE: std::sync::OnceLock<
        std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    > = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        let seeded: std::collections::HashSet<String> = load_probe_cache()
            .responses_endpoint_missing
            .into_iter()
            .collect();
        std::sync::Arc::new(std::sync::RwLock::new(seeded))
    })
}
