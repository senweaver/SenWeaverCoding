// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{
    StreamChunk, StreamError, StreamEvent, StreamResult, TokenUsage,
    ToolCall as ProviderToolCall,
};
use futures_util::{StreamExt, stream};
use serde::Deserialize;

const MAX_STREAM_TOOL_CALLS: usize = 1024;
pub(crate) const MAX_STREAM_TOOL_ARGS_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_STREAM_TOOL_ARGS_TOTAL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct StreamChunkResponse {

    #[serde(default)]
    pub choices: Vec<StreamChoice>,

    #[serde(default)]
    pub usage: Option<StreamUsageInfo>,

    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

pub fn stream_error_text(chunk: &StreamChunkResponse) -> Option<String> {
    let err = chunk.error.as_ref()?;
    if err.is_null() {
        return None;
    }
    if let Some(text) = err.as_str() {
        return Some(format!("provider stream error: {text}"));
    }
    let message = err
        .get("message")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| err.to_string());
    let code = err.get("code").and_then(|v| {
        v.as_str()
            .map(str::to_string)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    });
    Some(match code {
        Some(code) => format!("provider stream error (code {code}): {message}"),
        None => format!("provider stream error: {message}"),
    })
}

pub fn stream_error_code(chunk: &StreamChunkResponse) -> Option<String> {
    let err = chunk.error.as_ref()?;
    if err.is_null() || err.is_string() {
        return None;
    }
    let candidates = [err.get("type"), err.get("code"), err.get("status")];
    for candidate in candidates.into_iter().flatten() {
        if let Some(s) = candidate.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_ascii_lowercase());
            }
        }
        if let Some(n) = candidate.as_u64() {
            return Some(n.to_string());
        }
    }
    None
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct StreamUsageInfo {
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens: Option<u64>,

    #[serde(default)]
    pub prompt_tokens_details: Option<StreamUsagePromptDetails>,

    #[serde(default)]
    pub completion_tokens_details: Option<StreamUsageCompletionDetails>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct StreamUsagePromptDetails {
    #[serde(default)]
    pub cached_tokens: Option<u64>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct StreamUsageCompletionDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
}

impl StreamUsageInfo {

    pub fn into_token_usage(self) -> Option<TokenUsage> {
        let prompt = self.prompt_tokens;
        let completion = self.completion_tokens;
        let cached = self
            .prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens);
        let reasoning = self
            .completion_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens);
        if prompt.is_none() && completion.is_none() && cached.is_none() {
            return None;
        }
        let any_nonzero = prompt.unwrap_or(0) > 0
            || completion.unwrap_or(0) > 0
            || cached.unwrap_or(0) > 0;
        if !any_nonzero {
            return None;
        }
        Some(TokenUsage {
            input_tokens: prompt,
            output_tokens: completion,
            cached_input_tokens: cached,
            cache_creation_input_tokens: None,
            reasoning_tokens: reasoning,
        })
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct StreamAssistantMessage {
    #[serde(default)]
    pub content: Option<serde_json::Value>,

    #[serde(default)]
    pub reasoning_content: Option<serde_json::Value>,

    #[serde(default)]
    pub reasoning: Option<serde_json::Value>,

    #[serde(default)]
    pub thinking: Option<serde_json::Value>,

    #[serde(default)]
    pub thinking_content: Option<serde_json::Value>,
}

impl StreamAssistantMessage {
    fn reasoning_text(&self) -> Option<String> {
        let mut buf = String::new();
        let (_, from_content) = self
            .content
            .as_ref()
            .map(split_content_value)
            .unwrap_or_default();
        buf.push_str(&from_content);
        if let Some(text) = first_reasoning_text([
            &self.reasoning_content,
            &self.reasoning,
            &self.thinking,
            &self.thinking_content,
        ]) {
            buf.push_str(&text);
        }
        nonempty_text(buf)
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct StreamChoice {
    #[serde(default)]
    pub delta: StreamDelta,

    #[serde(default)]
    pub message: Option<StreamAssistantMessage>,

    #[serde(default)]
    pub finish_reason: Option<String>,

    #[serde(default)]
    pub reasoning_content: Option<serde_json::Value>,

    #[serde(default)]
    pub reasoning: Option<serde_json::Value>,
}

impl StreamChoice {
    fn choice_reasoning_text(&self) -> Option<String> {
        first_reasoning_text([&self.reasoning_content, &self.reasoning])
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct StreamDelta {
    #[serde(default)]
    pub content: Option<serde_json::Value>,

    #[serde(default)]
    pub reasoning_content: Option<serde_json::Value>,

    #[serde(default)]
    pub reasoning: Option<serde_json::Value>,

    #[serde(default, rename = "chain_of_thought")]
    pub chain_of_thought: Option<serde_json::Value>,

    #[serde(default)]
    pub reasoning_details: Option<serde_json::Value>,

    #[serde(default, alias = "reasoning_text", alias = "thought")]
    pub thinking: Option<serde_json::Value>,

    #[serde(default)]
    pub thinking_content: Option<serde_json::Value>,

    #[serde(default)]
    pub tool_calls: Option<serde_json::Value>,
}

impl StreamDelta {
    pub fn visible_and_reasoning(&self) -> (Option<String>, Option<String>) {
        let (visible, from_content) = self
            .content
            .as_ref()
            .map(split_content_value)
            .unwrap_or_default();
        let mut reasoning = from_content;
        if let Some(text) = first_reasoning_text([
            &self.reasoning_content,
            &self.reasoning,
            &self.thinking,
            &self.thinking_content,
            &self.chain_of_thought,
            &self.reasoning_details,
        ]) {
            reasoning.push_str(&text);
        }
        (nonempty_text(visible), nonempty_text(reasoning))
    }

    pub fn content_text(&self) -> Option<String> {
        self.visible_and_reasoning().0
    }

    pub fn reasoning_text(&self) -> Option<String> {
        self.visible_and_reasoning().1
    }

    pub fn tool_call_deltas(&self) -> Vec<StreamToolCallDelta> {
        let Some(value) = &self.tool_calls else {
            return Vec::new();
        };
        match value {
            serde_json::Value::Array(items) => {
                items.iter().map(salvage_tool_call_delta).collect()
            }
            other => vec![salvage_tool_call_delta(other)],
        }
    }
}

fn coerce_delta_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn salvage_tool_call_delta(item: &serde_json::Value) -> StreamToolCallDelta {
    if let Ok(parsed) = serde_json::from_value::<StreamToolCallDelta>(item.clone()) {
        return parsed;
    }
    let obj = item.as_object();
    let field = |key: &str| obj.and_then(|o| o.get(key)).and_then(coerce_delta_string);
    let index = obj.and_then(|o| o.get("index")).and_then(|v| {
        v.as_u64()
            .map(|n| n as usize)
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<usize>().ok()))
    });
    let function = obj
        .and_then(|o| o.get("function"))
        .and_then(|f| f.as_object())
        .map(|f| StreamFunctionDelta {
            name: f.get("name").and_then(coerce_delta_string),
            arguments: f.get("arguments").and_then(coerce_delta_string),
        });
    StreamToolCallDelta {
        index,
        id: field("id"),
        function,
        name: field("name"),
        arguments: field("arguments"),
    }
}

fn nonempty_text(text: String) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn is_reasoning_part_type(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("think") || kind.contains("reason") || kind.contains("thought")
}

fn object_part_text(map: &serde_json::Map<String, serde_json::Value>) -> String {
    for key in ["text", "content", "summary", "thinking", "reasoning", "reasoning_content"] {
        if let Some(value) = map.get(key) {
            if value.is_object() || value.is_array() {
                let (visible, reasoning) = split_content_value(value);
                let mut buf = visible;
                buf.push_str(&reasoning);
                if !buf.is_empty() {
                    return buf;
                }
                continue;
            }
            if let Some(text) = value.as_str() {
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }
    }
    String::new()
}

pub(crate) fn split_content_value(value: &serde_json::Value) -> (String, String) {
    match value {
        serde_json::Value::Null => (String::new(), String::new()),
        serde_json::Value::String(text) => (text.clone(), String::new()),
        serde_json::Value::Array(items) => {
            let mut visible = String::new();
            let mut reasoning = String::new();
            for item in items {
                match item {
                    serde_json::Value::String(text) => visible.push_str(text),
                    serde_json::Value::Object(map) => {
                        let kind = map
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let text = object_part_text(map);
                        if is_reasoning_part_type(kind) {
                            reasoning.push_str(&text);
                        } else {
                            visible.push_str(&text);
                        }
                    }
                    serde_json::Value::Array(_) => {
                        let (v, r) = split_content_value(item);
                        visible.push_str(&v);
                        reasoning.push_str(&r);
                    }
                    _ => {}
                }
            }
            (visible, reasoning)
        }
        serde_json::Value::Object(map) => {
            let kind = map
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let text = object_part_text(map);
            if is_reasoning_part_type(kind) {
                (String::new(), text)
            } else {
                (text, String::new())
            }
        }
        _ => (String::new(), String::new()),
    }
}

pub(crate) fn value_to_plain_text(value: &serde_json::Value) -> Option<String> {
    let (visible, reasoning) = split_content_value(value);
    let mut buf = visible;
    buf.push_str(&reasoning);
    nonempty_text(buf)
}

fn first_reasoning_text<'a, const N: usize>(
    values: [&'a Option<serde_json::Value>; N],
) -> Option<String> {
    for value in values {
        if let Some(text) = reasoning_value_to_string(value.as_ref()) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn reasoning_value_to_string(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => nonempty_text(text.clone()),
        other => {
            let (visible, reasoning) = split_content_value(other);
            let mut buf = visible;
            buf.push_str(&reasoning);
            nonempty_text(buf)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct StreamToolCallDelta {
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<StreamFunctionDelta>,

    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamFunctionDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Default)]
pub struct StreamToolCallAccumulator {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    arguments_overflow: bool,
}

impl StreamToolCallAccumulator {

    pub fn apply_delta(&mut self, delta: &StreamToolCallDelta, remaining_total_budget: usize) -> usize {
        if let Some(id) = delta.id.as_ref().filter(|value| !value.is_empty()) {
            self.id = Some(id.clone());
        }

        let delta_name = delta
            .function
            .as_ref()
            .and_then(|function| function.name.as_ref())
            .or(delta.name.as_ref())
            .filter(|value| !value.is_empty());
        if let Some(name) = delta_name {
            self.name = Some(name.clone());
        }

        if let Some(arguments_delta) = delta
            .function
            .as_ref()
            .and_then(|function| function.arguments.as_ref())
            .or(delta.arguments.as_ref())
            .filter(|value| !value.is_empty())
        {
            let per_call_room = MAX_STREAM_TOOL_ARGS_BYTES.saturating_sub(self.arguments.len());
            let room = per_call_room.min(remaining_total_budget);
            if arguments_delta.len() <= room {
                self.arguments.push_str(arguments_delta);
                return arguments_delta.len();
            } else if !self.arguments_overflow {
                self.arguments_overflow = true;
                tracing::warn!(
                    target: "providers.openai_sse",
                    per_call_limit = MAX_STREAM_TOOL_ARGS_BYTES,
                    total_limit = MAX_STREAM_TOOL_ARGS_TOTAL_BYTES,
                    "stream tool_call arguments exceeded size limit; truncating"
                );
            }
        }
        0
    }

    #[must_use]
    pub fn current_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn current_id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    #[must_use]
    pub fn args_len(&self) -> usize {
        self.arguments.len()
    }

    fn is_empty_slot(&self) -> bool {
        self.id.is_none() && self.name.is_none() && self.arguments.is_empty()
    }

    fn is_complete(&self) -> bool {
        if self.arguments_overflow {
            return false;
        }
        let Some(name) = self.name.as_deref() else {
            return false;
        };
        if name.is_empty() {
            return false;
        }
        let arguments = self.arguments.trim();
        if arguments.is_empty() {
            return true;
        }
        serde_json::from_str::<serde_json::Value>(arguments).is_ok()
    }

    pub fn into_provider_tool_call(
        self,
        kind: crate::providers::sanitize::ProviderKind,
    ) -> Option<ProviderToolCall> {
        let name = self.name?;
        let normalized_arguments =
            crate::providers::sanitize::normalize_tool_call_arguments(&name, self.arguments);

        Some(ProviderToolCall {
            id: crate::providers::sanitize::normalize_tool_call_id_for_provider(self.id, kind),
            name,
            arguments: normalized_arguments,
        })
    }
}

pub fn parse_sse_chunk(line: &str) -> StreamResult<Option<StreamChunkResponse>> {
    let line = line.trim();

    if line.is_empty() || line.starts_with(':') {
        return Ok(None);
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();

    if data == "[DONE]" {
        return Ok(None);
    }

    match serde_json::from_str::<StreamChunkResponse>(data) {
        Ok(value) => Ok(Some(value)),
        Err(err) => {
            if let Ok(root) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(salvaged) = salvage_stream_chunk(&root) {
                    return Ok(Some(salvaged));
                }
            }
            Err(StreamError::Json(err))
        }
    }
}

fn salvage_choice_delta(delta: &serde_json::Value) -> StreamDelta {
    if delta.is_string() {
        return StreamDelta {
            content: Some(delta.clone()),
            ..StreamDelta::default()
        };
    }
    serde_json::from_value(delta.clone()).unwrap_or_else(|_| {
        let mut fallback = StreamDelta::default();
        if let Some(obj) = delta.as_object() {
            fallback.content = obj.get("content").cloned();
            fallback.reasoning_content = obj.get("reasoning_content").cloned();
            fallback.reasoning = obj.get("reasoning").cloned();
            fallback.thinking = obj.get("thinking").cloned();
            fallback.thinking_content = obj.get("thinking_content").cloned();
            fallback.chain_of_thought = obj
                .get("chain_of_thought")
                .cloned()
                .or_else(|| obj.get("reasoning_details").cloned());
            fallback.reasoning_details = obj.get("reasoning_details").cloned();
            fallback.tool_calls = obj.get("tool_calls").cloned();
        }
        fallback
    })
}

fn salvage_stream_choice(item: &serde_json::Value) -> StreamChoice {
    let delta = item
        .get("delta")
        .map(salvage_choice_delta)
        .unwrap_or_default();
    let message = item
        .get("message")
        .and_then(|m| serde_json::from_value(m.clone()).ok());
    StreamChoice {
        delta,
        message,
        finish_reason: item
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        reasoning_content: item.get("reasoning_content").cloned(),
        reasoning: item.get("reasoning").cloned(),
    }
}

fn salvage_stream_chunk(root: &serde_json::Value) -> Option<StreamChunkResponse> {
    if let Ok(parsed) = serde_json::from_value::<StreamChunkResponse>(root.clone()) {
        return Some(parsed);
    }
    let mut choices = Vec::new();
    if let Some(arr) = root.get("choices").and_then(|v| v.as_array()) {
        choices.extend(arr.iter().map(salvage_stream_choice));
    } else if root.get("delta").is_some() || root.get("content").is_some() {
        choices.push(salvage_stream_choice(root));
    }
    let usage = root
        .get("usage")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());
    let error = root.get("error").cloned();
    if choices.is_empty() && usage.is_none() && error.as_ref().is_none_or(serde_json::Value::is_null)
    {
        return None;
    }
    Some(StreamChunkResponse {
        choices,
        usage,
        error,
    })
}

pub fn parse_sse_chunk_tolerant(line: &str) -> Option<StreamChunkResponse> {
    match parse_sse_chunk(line) {
        Ok(value) => value,
        Err(err) => {
            let preview: String = line.chars().take(160).collect();
            tracing::warn!(
                target: "providers.core.openai_sse",
                error = %err,
                line_preview = %preview,
                "skipped malformed SSE chunk; continuing to next event"
            );
            None
        }
    }
}

pub fn parse_proxy_tool_event(line: &str) -> Option<StreamEvent> {
    let data = line.trim().strip_prefix("data:")?.trim();
    let obj: serde_json::Value = serde_json::from_str(data).ok()?;

    if let Some(ts) = obj.get("x_tool_start") {
        let Some(name) = ts.get("name").and_then(|v| v.as_str()) else {
            tracing::debug!("proxy x_tool_start event missing required 'name' field");
            return None;
        };
        let name = name.to_string();
        let args = ts
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("{}")
            .to_string();
        return Some(StreamEvent::PreExecutedToolCall { name, args });
    }

    if let Some(tr) = obj.get("x_tool_result") {
        let name = tr
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let output = tr
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(StreamEvent::PreExecutedToolResult { name, output });
    }

    None
}

pub fn extract_sse_text_delta(choice: &StreamChoice) -> Option<String> {
    choice.delta.content_text()
}

pub fn sse_bytes_to_chunks(
    response: reqwest::Response,
    count_tokens: bool,
) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

    let _ = crate::runtime::spawn_supervised(
        "providers.core.openai_sse.sse_bytes_to_chunks",
        async move {
            let mut sse = super::sse::SseParser::new();
            let mut saw_terminator = false;
            let mut made_progress = false;

            match response.error_for_status_ref() {
                Ok(_) => {}
                Err(e) => {
                    let _ = tx.send(Err(StreamError::Http(e))).await;
                    return;
                }
            }

            let mut bytes_stream = response.bytes_stream();
            let mut stream_ended = false;
            while !stream_ended {
                match bytes_stream.next().await {
                    Some(Ok(bytes)) => {
                        sse.push(&bytes);
                        if sse.overflowed() {
                            let _ = tx
                                .send(Err(StreamError::Provider(
                                    "SSE line exceeded size limit; upstream response malformed or truncated".to_string(),
                                )))
                                .await;
                            return;
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                    None => {
                        sse.finish();
                        stream_ended = true;
                    }
                }

                while let Some(ev) = sse.next_event() {
                    if ev.is_done() {
                        saw_terminator = true;
                        continue;
                    }
                    if ev.data.is_empty() {
                        continue;
                    }
                    let line = format!("data: {}", ev.data);
                    let Some(parsed) = parse_sse_chunk_tolerant(&line) else {
                        continue;
                    };
                    if let Some(err_text) = stream_error_text(&parsed) {
                        let _ = tx
                            .send(Err(crate::providers::stream_declared_error(
                                "upstream",
                                stream_error_code(&parsed),
                                err_text,
                            )))
                            .await;
                        return;
                    }
                    if parsed.choices.iter().any(|c| c.finish_reason.is_some()) {
                        saw_terminator = true;
                    }
                    if let Some(chunk) = chunk_text_from_response(&parsed) {
                        made_progress = true;
                        let chunk = if count_tokens {
                            chunk.with_token_estimate()
                        } else {
                            chunk
                        };
                        if tx.send(Ok(chunk)).await.is_err() {
                            return;
                        }
                    }
                }
            }

            if !made_progress && !saw_terminator {
                let _ = tx
                    .send(Err(StreamError::Provider(
                        "upstream stream closed before completion (no [DONE]/finish_reason); connection closed mid-response".to_string(),
                    )))
                    .await;
                return;
            }
            if made_progress && !saw_terminator {
                tracing::warn!(
                    target: "provider.stream",
                    "upstream stream closed without [DONE]/finish_reason after partial output; failing closed so the turn is retried instead of presenting truncated output as complete"
                );
                let _ = tx
                    .send(Err(StreamError::Provider(
                        "upstream stream closed without [DONE]/finish_reason after partial output; connection closed mid-response".to_string(),
                    )))
                    .await;
                return;
            }

            let _ = tx.send(Ok(StreamChunk::final_chunk())).await;
        },
    );

    stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|chunk| (chunk, rx))
    })
    .boxed()
}

pub fn sse_bytes_to_events(
    response: reqwest::Response,
    count_tokens: bool,
    provider_kind: crate::providers::sanitize::ProviderKind,
) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

    let _ = crate::runtime::spawn_supervised(
        "providers.core.openai_sse.sse_bytes_to_events",
        async move {
            let mut sse = super::sse::SseParser::new();
            let mut tool_calls: Vec<StreamToolCallAccumulator> = Vec::new();
            let mut emitted_tool_calls = false;
            let mut total_tool_args_bytes: usize = 0;
            let mut saw_terminator = false;
            let mut made_progress = false;
            let mut saw_text_content = false;
            let mut saw_reasoning_content = false;

            match response.error_for_status_ref() {
                Ok(_) => {}
                Err(e) => {
                    let _ = tx.send(Err(StreamError::Http(e))).await;
                    return;
                }
            }

            let mut bytes_stream = response.bytes_stream();
            let mut stream_ended = false;
            while !stream_ended {
                match bytes_stream.next().await {
                    Some(Ok(bytes)) => {
                        sse.push(&bytes);
                        if sse.overflowed() {
                            let _ = tx
                                .send(Err(StreamError::Provider(
                                    "SSE line exceeded size limit; upstream response malformed or truncated".to_string(),
                                )))
                                .await;
                            return;
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                    None => {
                        sse.finish();
                        stream_ended = true;
                    }
                }

                while let Some(ev) = sse.next_event() {
                            if ev.is_done() {
                                saw_terminator = true;
                                continue;
                            }
                            if ev.data.is_empty() {
                                continue;
                            }
                            let line = format!("data: {}", ev.data);

                            if let Some(event) = parse_proxy_tool_event(&line) {
                                made_progress = true;
                                if tx.send(Ok(event)).await.is_err() {
                                    return;
                                }
                                continue;
                            }

                            let chunk = match parse_sse_chunk_tolerant(&line) {
                                Some(chunk) => chunk,
                                None => continue,
                            };

                            if let Some(err_text) = stream_error_text(&chunk) {
                                tracing::warn!(
                                    target: "provider.stream",
                                    error = %err_text,
                                    "upstream emitted in-stream error event; failing the turn instead of masking it as a complete response"
                                );
                                let _ = tx
                                    .send(Err(crate::providers::stream_declared_error(
                                        "upstream",
                                        stream_error_code(&chunk),
                                        err_text,
                                    )))
                                    .await;
                                return;
                            }

                            if let Some(usage_info) = chunk.usage.clone() {
                                if let Some(usage) = usage_info.into_token_usage() {
                                    made_progress = true;
                                    if tx
                                        .send(Ok(StreamEvent::Usage(usage)))
                                        .await
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }

                            let mut should_emit_tool_calls = false;
                            for choice in &chunk.choices {
                                let (visible, mut reasoning_text) = choice.delta.visible_and_reasoning();
                                if reasoning_text.is_none() {
                                    reasoning_text = choice.choice_reasoning_text();
                                }
                                if reasoning_text.is_none() && !saw_reasoning_content {
                                    reasoning_text = choice
                                        .message
                                        .as_ref()
                                        .and_then(StreamAssistantMessage::reasoning_text);
                                }

                                if let Some(reasoning) = reasoning_text {
                                    if !reasoning.is_empty() {
                                        made_progress = true;
                                        saw_reasoning_content = true;
                                        let reasoning_chunk = StreamChunk::reasoning(reasoning);
                                        if tx
                                            .send(Ok(StreamEvent::TextDelta(reasoning_chunk)))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }

                                if let Some(content) = visible {
                                    if !content.is_empty() {
                                        made_progress = true;
                                        saw_text_content = true;
                                        let mut text_chunk = StreamChunk::delta(content);
                                        if count_tokens {
                                            text_chunk = text_chunk.with_token_estimate();
                                        }
                                        if tx
                                            .send(Ok(StreamEvent::TextDelta(text_chunk)))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }

                                let deltas = choice.delta.tool_call_deltas();
                                if !deltas.is_empty() {
                                    for delta in &deltas {
                                        let index = match delta.index {
                                            Some(i) => i,
                                            None => {
                                                let delta_id = delta
                                                    .id
                                                    .as_deref()
                                                    .filter(|v| !v.is_empty());
                                                let existing_by_id = delta_id.and_then(|id| {
                                                    tool_calls
                                                        .iter()
                                                        .position(|acc| acc.current_id() == Some(id))
                                                });
                                                if let Some(existing) = existing_by_id {
                                                    existing
                                                } else {
                                                    let starts_new_call = delta_id.is_some()
                                                        || delta
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.name.as_deref())
                                                            .or(delta.name.as_deref())
                                                            .is_some_and(|v| !v.is_empty());
                                                    if tool_calls.is_empty() {
                                                        0
                                                    } else if starts_new_call {
                                                        tool_calls.len()
                                                    } else {
                                                        tool_calls.len() - 1
                                                    }
                                                }
                                            }
                                        };
                                        if index >= MAX_STREAM_TOOL_CALLS {
                                            tracing::warn!(
                                                index,
                                                max = MAX_STREAM_TOOL_CALLS,
                                                "ignoring streamed tool_call with out-of-range index"
                                            );
                                            continue;
                                        }
                                        if index >= tool_calls.len() {
                                            tool_calls.resize_with(index + 1, Default::default);
                                        }
                                        if let Some(acc) = tool_calls.get_mut(index) {
                                            let remaining = MAX_STREAM_TOOL_ARGS_TOTAL_BYTES
                                                .saturating_sub(total_tool_args_bytes);
                                            let consumed = acc.apply_delta(delta, remaining);
                                            total_tool_args_bytes =
                                                total_tool_args_bytes.saturating_add(consumed);
                                            if consumed > 0 {
                                                let args_total_len =
                                                    acc.args_len().min(u32::MAX as usize) as u32;
                                                if let Some(name) = acc.current_name() {
                                                    let args_delta = delta
                                                        .function
                                                        .as_ref()
                                                        .and_then(|f| f.arguments.as_deref())
                                                        .or(delta.arguments.as_deref())
                                                        .unwrap_or_default()
                                                        .to_string();
                                                    if !args_delta.is_empty()
                                                        && tx
                                                            .send(Ok(
                                                                StreamEvent::ToolCallArgsDelta {
                                                                    call_index: index as u32,
                                                                    name: name.to_string(),
                                                                    args_delta,
                                                                    args_total_len,
                                                                },
                                                            ))
                                                            .await
                                                            .is_err()
                                                    {
                                                        return;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                if choice.finish_reason.is_some() {
                                    saw_terminator = true;
                                }
                                if let Some(reason) = choice
                                    .finish_reason
                                    .as_deref()
                                    .and_then(crate::providers::traits::StopReason::from_wire)
                                {
                                    if tx
                                        .send(Ok(StreamEvent::StopReason(reason)))
                                        .await
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                                if matches!(
                                    choice
                                        .finish_reason
                                        .as_deref()
                                        .and_then(crate::providers::traits::StopReason::from_wire),
                                    Some(crate::providers::traits::StopReason::ToolCalls)
                                ) {
                                    should_emit_tool_calls = true;
                                }
                            }

                            if should_emit_tool_calls && !emitted_tool_calls {
                                emitted_tool_calls = true;
                                for tool_call in tool_calls
                                    .drain(..)
                                    .filter_map(|acc| acc.into_provider_tool_call(provider_kind))
                                {
                                    made_progress = true;
                                    if tx.send(Ok(StreamEvent::ToolCall(tool_call))).await.is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        }
            }

            let tool_calls_fully_received = match tool_calls
                .iter()
                .rposition(|acc| !acc.is_empty_slot())
            {
                Some(last) => tool_calls[..=last]
                    .iter()
                    .all(StreamToolCallAccumulator::is_complete),
                None => false,
            };
            let no_pending_tool_calls = tool_calls.is_empty() || tool_calls_fully_received;

            if !saw_terminator
                && !emitted_tool_calls
                && !tool_calls.is_empty()
                && !tool_calls_fully_received
            {
                let _ = tx
                    .send(Err(StreamError::Provider(
                        "upstream stream closed before completion (no [DONE]/finish_reason) while tool_call arguments were still streaming; connection closed mid-response".to_string(),
                    )))
                    .await;
                return;
            }

            if emitted_tool_calls && tool_calls.iter().any(|acc| !acc.is_empty_slot()) {
                tracing::warn!(
                    late = tool_calls.len(),
                    "tool_call deltas arrived after finish_reason emit; emitting them as additional calls"
                );
            }
            for tool_call in tool_calls
                .drain(..)
                .filter_map(|acc| acc.into_provider_tool_call(provider_kind))
            {
                made_progress = true;
                if tx.send(Ok(StreamEvent::ToolCall(tool_call))).await.is_err() {
                    return;
                }
            }

            if !made_progress && !saw_terminator {
                let _ = tx
                    .send(Err(StreamError::Provider(
                        "upstream stream closed before completion (no [DONE]/finish_reason); connection closed mid-response".to_string(),
                    )))
                    .await;
                return;
            }

            let clean_non_text_finish = !saw_text_content
                && no_pending_tool_calls
                && (saw_reasoning_content || tool_calls_fully_received);

            if made_progress && !saw_terminator && !clean_non_text_finish {
                tracing::warn!(
                    target: "provider.stream",
                    "upstream stream closed without [DONE]/finish_reason after partial output; failing closed so the turn is retried instead of presenting truncated output as complete"
                );
                let _ = tx
                    .send(Err(StreamError::Provider(
                        "upstream stream closed without [DONE]/finish_reason after partial output; connection closed mid-response".to_string(),
                    )))
                    .await;
                return;
            }

            let _ = tx.send(Ok(StreamEvent::Final)).await;
        },
    );

    stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (event, rx))
    })
    .boxed()
}

fn chunk_text_from_response(chunk: &StreamChunkResponse) -> Option<StreamChunk> {
    let choice = chunk.choices.first()?;
    let (content, mut reasoning) = choice.delta.visible_and_reasoning();
    if reasoning.is_none() {
        reasoning = choice.choice_reasoning_text();
    }
    if reasoning.is_none() {
        reasoning = choice
            .message
            .as_ref()
            .and_then(StreamAssistantMessage::reasoning_text);
    }
    match (content.as_deref(), reasoning.as_deref()) {
        (None, None) => None,
        (content, reasoning) => Some(StreamChunk {
            delta: content.unwrap_or("").to_string(),
            reasoning: reasoning.map(str::to_string),
            is_final: false,
            token_count: 0,
        }),
    }
}

