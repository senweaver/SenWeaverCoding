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
const MAX_STREAM_TOOL_ARGS_BYTES: usize = 64 * 1024 * 1024;
const MAX_STREAM_TOOL_ARGS_TOTAL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct StreamChunkResponse {

    #[serde(default)]
    pub choices: Vec<StreamChoice>,

    #[serde(default)]
    pub usage: Option<StreamUsageInfo>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct StreamUsageInfo {
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens: Option<u64>,

    #[serde(default)]
    pub prompt_tokens_details: Option<StreamUsagePromptDetails>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct StreamUsagePromptDetails {
    #[serde(default)]
    pub cached_tokens: Option<u64>,
}

impl StreamUsageInfo {

    pub fn into_token_usage(self) -> Option<TokenUsage> {
        let prompt = self.prompt_tokens;
        let completion = self.completion_tokens;
        let cached = self
            .prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens);
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
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    #[serde(default)]
    pub delta: StreamDelta,

    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct StreamDelta {
    #[serde(default)]
    pub content: Option<String>,

    #[serde(
        default,
        alias = "reasoning",
        alias = "thinking",
        alias = "thinking_content",
        alias = "chain_of_thought",
        deserialize_with = "deserialize_reasoning_content"
    )]
    pub reasoning_content: Option<String>,

    #[serde(default)]
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

fn deserialize_reasoning_content<'de, D>(de: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let value = serde_json::Value::deserialize(de)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(text) => Ok(Some(text)),
        serde_json::Value::Object(map) => {
            for key in [
                "text",
                "content",
                "thinking",
                "reasoning",
                "reasoning_content",
            ] {
                if let Some(text) = map.get(key).and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        return Ok(Some(text.to_string()));
                    }
                }
            }
            Ok(None)
        }
        serde_json::Value::Array(items) => {
            let mut buf = String::new();
            for item in items {
                if let Some(s) = item.as_str() {
                    buf.push_str(s);
                    continue;
                }
                if let Some(map) = item.as_object() {
                    for key in ["text", "content", "thinking", "reasoning"] {
                        if let Some(text) = map.get(key).and_then(|v| v.as_str()) {
                            buf.push_str(text);
                            break;
                        }
                    }
                }
            }
            if buf.is_empty() { Ok(None) } else { Ok(Some(buf)) }
        }
        _ => Ok(None),
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

    pub fn into_provider_tool_call(self) -> Option<ProviderToolCall> {
        let name = self.name?;
        let arguments = if self.arguments.trim().is_empty() {
            "{}".to_string()
        } else {
            self.arguments
        };
        let normalized_arguments = if serde_json::from_str::<serde_json::Value>(&arguments).is_ok()
        {
            arguments
        } else if let Some(repaired) =
            crate::providers::sanitize::repair_partial_tool_input_json(&arguments)
        {
            tracing::warn!(
                function = %name,
                arguments_len = arguments.len(),
                repaired_len = repaired.len(),
                "streamed native tool-call arguments were truncated; recovered partial arguments via structural repair"
            );
            repaired
        } else {
            tracing::warn!(
                function = %name,
                arguments = %arguments,
                "Invalid JSON in streamed native tool-call arguments, using empty object"
            );
            "{}".to_string()
        };

        Some(ProviderToolCall {
            id: crate::providers::sanitize::normalize_tool_call_id(self.id),
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

    serde_json::from_str(data)
        .map(Some)
        .map_err(StreamError::Json)
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
    if let Some(content) = &choice.delta.content {
        if !content.is_empty() {
            return Some(content.clone());
        }
    }

    choice
        .delta
        .reasoning_content
        .as_ref()
        .filter(|value| !value.is_empty())
        .cloned()
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

            match response.error_for_status_ref() {
                Ok(_) => {}
                Err(e) => {
                    let _ = tx.send(Err(StreamError::Http(e))).await;
                    return;
                }
            }

            let mut bytes_stream = response.bytes_stream();

            while let Some(item) = bytes_stream.next().await {
                match item {
                    Ok(bytes) => {
                        sse.push(&bytes);
                        if sse.overflowed() {
                            let _ = tx
                                .send(Err(StreamError::Provider(
                                    "SSE line exceeded size limit; upstream response malformed or truncated".to_string(),
                                )))
                                .await;
                            return;
                        }
                        while let Some(ev) = sse.next_event() {
                            if ev.is_done() || ev.data.is_empty() {
                                continue;
                            }
                            let line = format!("data: {}", ev.data);
                            if let Some(chunk) = parse_sse_line_tolerant(&line) {
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
                    Err(e) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                }
            }
            sse.finish();
            while let Some(ev) = sse.next_event() {
                if ev.is_done() || ev.data.is_empty() {
                    continue;
                }
                let line = format!("data: {}", ev.data);
                if let Some(chunk) = parse_sse_line_tolerant(&line) {
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

            match response.error_for_status_ref() {
                Ok(_) => {}
                Err(e) => {
                    let _ = tx.send(Err(StreamError::Http(e))).await;
                    return;
                }
            }

            let mut bytes_stream = response.bytes_stream();
            while let Some(item) = bytes_stream.next().await {
                match item {
                    Ok(bytes) => {
                        sse.push(&bytes);
                        if sse.overflowed() {
                            let _ = tx
                                .send(Err(StreamError::Provider(
                                    "SSE line exceeded size limit; upstream response malformed or truncated".to_string(),
                                )))
                                .await;
                            return;
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

                                if let Some(content) = &choice.delta.content {
                                    if !content.is_empty() {
                                        made_progress = true;
                                        let mut text_chunk = StreamChunk::delta(content.clone());
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
                                if let Some(reasoning) = &choice.delta.reasoning_content {
                                    if !reasoning.is_empty() {
                                        made_progress = true;
                                        let reasoning_chunk =
                                            StreamChunk::reasoning(reasoning.clone());
                                        if tx
                                            .send(Ok(StreamEvent::TextDelta(reasoning_chunk)))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }

                                if let Some(deltas) = choice.delta.tool_calls.as_ref() {
                                    for delta in deltas {
                                        let index = delta.index.unwrap_or(tool_calls.len());
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
                                            total_tool_args_bytes = total_tool_args_bytes
                                                .saturating_add(acc.apply_delta(delta, remaining));
                                        }
                                    }
                                }

                                if choice.finish_reason.is_some() {
                                    saw_terminator = true;
                                }
                                if choice.finish_reason.as_deref() == Some("tool_calls") {
                                    should_emit_tool_calls = true;
                                }
                            }

                            if should_emit_tool_calls && !emitted_tool_calls {
                                emitted_tool_calls = true;
                                for tool_call in tool_calls
                                    .drain(..)
                                    .filter_map(StreamToolCallAccumulator::into_provider_tool_call)
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
                    Err(e) => {
                        let _ = tx.send(Err(StreamError::Http(e))).await;
                        return;
                    }
                }
            }

            if !emitted_tool_calls {
                for tool_call in tool_calls
                    .drain(..)
                    .filter_map(StreamToolCallAccumulator::into_provider_tool_call)
                {
                    made_progress = true;
                    if tx.send(Ok(StreamEvent::ToolCall(tool_call))).await.is_err() {
                        return;
                    }
                }
            }

            if made_progress && !saw_terminator {
                let _ = tx
                    .send(Err(StreamError::Provider(
                        "upstream stream closed before completion (no [DONE]/finish_reason); connection closed mid-response".to_string(),
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

fn parse_sse_line(line: &str) -> StreamResult<Option<StreamChunk>> {
    let chunk = match parse_sse_chunk(line)? {
        Some(c) => c,
        None => return Ok(None),
    };

    if let Some(choice) = chunk.choices.first() {
        if let Some(content) = &choice.delta.content {
            if !content.is_empty() {
                return Ok(Some(StreamChunk::delta(content.clone())));
            }
        }
        if let Some(reasoning) = &choice.delta.reasoning_content {
            if !reasoning.is_empty() {
                return Ok(Some(StreamChunk::reasoning(reasoning.clone())));
            }
        }
    }

    Ok(None)
}

fn parse_sse_line_tolerant(line: &str) -> Option<StreamChunk> {
    match parse_sse_line(line) {
        Ok(value) => value,
        Err(err) => {
            let preview: String = line.chars().take(160).collect();
            tracing::warn!(
                target: "providers.core.openai_sse",
                error = %err,
                line_preview = %preview,
                "skipped malformed SSE chunk line; continuing"
            );
            None
        }
    }
}
