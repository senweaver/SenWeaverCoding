// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, StructuredResponse, ToolCall as ProviderToolCall, TokenUsage,
    parse_first_json_object,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub const RESPONSES_API_MODEL_PREFIXES: &[&str] = &[
    "gpt-5", "gpt-4.1", "gpt-4o", "o1", "o3", "o4",
];

#[must_use]
pub fn model_uses_responses_api(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    RESPONSES_API_MODEL_PREFIXES
        .iter()
        .any(|p| m.starts_with(p))
}

pub struct OpenAiResponsesProvider {
    base_url: String,
    credential: Option<String>,
    max_output_tokens: Option<u32>,
    reasoning_effort: Option<String>,
    extra_headers: std::collections::HashMap<String, String>,
}

impl OpenAiResponsesProvider {
    #[must_use]
    pub fn new(credential: Option<&str>) -> Self {
        Self::with_base_url(None, credential)
    }

    #[must_use]
    pub fn with_base_url(base_url: Option<&str>, credential: Option<&str>) -> Self {
        Self {
            base_url: base_url
                .map(|u| u.trim_end_matches('/').to_string())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            credential: credential.map(ToString::to_string),
            max_output_tokens: None,
            reasoning_effort: None,
            extra_headers: std::collections::HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_max_output_tokens(mut self, max_output_tokens: Option<u32>) -> Self {
        self.max_output_tokens = max_output_tokens;
        self
    }

    #[must_use]
    pub fn with_reasoning_effort(mut self, reasoning_effort: Option<String>) -> Self {
        self.reasoning_effort = reasoning_effort;
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

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_llm_chat_client(
                "provider.openai_responses",
                120,
                10,
                &self.extra_headers,
            )
    }

    fn is_reasoning_model(model: &str) -> bool {
        let m = model.trim().to_ascii_lowercase();
        m.starts_with("gpt-5") || m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4")
    }

    fn apply_temperature(body: &mut serde_json::Value, model: &str, requested: f64) {
        if !Self::is_reasoning_model(model) {
            body["temperature"] = serde_json::json!(requested);
        }
    }

    fn apply_reasoning(&self, body: &mut serde_json::Value, model: &str) {
        if !Self::is_reasoning_model(model) {
            return;
        }
        let effort = self
            .reasoning_effort
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("medium");
        body["reasoning"] = serde_json::json!({ "effort": effort });
    }

    fn apply_prompt_cache_key(&self, body: &mut serde_json::Value) {
        if !self.base_url.starts_with("https://api.openai.com") {
            return;
        }
        if let Some(key) = crate::session::current_session_context()
            .map(|ctx| format!("sen-{}", ctx.session_id))
            .filter(|k| k.len() > 4)
        {
            body["prompt_cache_key"] = serde_json::Value::String(key);
        }
    }

    async fn run_tools_request(
        &self,
        messages: &[ChatMessage],
        tools: Option<Vec<serde_json::Value>>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml \
                 (provider = openai-responses)."
            )
        })?;

        let (instructions, input_items) = build_responses_input_items(messages);

        let mut body = serde_json::json!({
            "model": model,
            "input": input_items,
        });
        Self::apply_temperature(&mut body, model, temperature);
        self.apply_reasoning(&mut body, model);
        self.apply_prompt_cache_key(&mut body);
        if let Some(instr) = instructions {
            body["instructions"] = serde_json::Value::String(instr);
        }
        if let Some(max) = self.max_output_tokens {
            body["max_output_tokens"] = serde_json::json!(max);
        }
        if let Some(tools) = tools {
            body["tools"] = serde_json::Value::Array(tools);
            body["tool_choice"] = serde_json::Value::String("auto".to_string());
        }

        let response = crate::providers::core::idempotency::apply_idempotency_header(
            self.http_client()
                .post(format!("{}/responses", self.base_url)),
        )
            .header("Authorization", format!("Bearer {credential}"))
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI Responses", response).await);
        }

        let parsed: ResponsesPayload = response.json().await?;
        parsed.ensure_not_failed()?;
        let usage = parsed.usage.as_ref().map(|u| {
            let cached = u
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens);
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, None);
            TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: None,
                reasoning_tokens: u
                    .output_tokens_details
                    .as_ref()
                    .and_then(|d| d.reasoning_tokens),
            }
        });
        let text = parsed.collect_text();
        let tool_calls = parsed.collect_tool_calls();
        let stop_reason = parsed.stop_reason(!tool_calls.is_empty());
        Ok(ProviderChatResponse {
            text: if text.is_empty() { None } else { Some(text) },
            tool_calls,
            usage,
            reasoning_content: None,
            thinking_signature: None,
            stop_reason,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ResponsesPayload {
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    output: Option<Vec<ResponsesOutputItem>>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    incomplete_details: Option<ResponsesIncompleteDetails>,
    #[serde(default)]
    error: Option<ResponsesError>,
}

#[derive(Debug, Deserialize)]
struct ResponsesIncompleteDetails {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    content: Option<Vec<ResponsesOutputContent>>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputContent {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    input_tokens_details: Option<ResponsesUsageDetails>,
    #[serde(default)]
    output_tokens_details: Option<ResponsesOutputTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsageDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

impl ResponsesPayload {
    fn ensure_not_failed(&self) -> anyhow::Result<()> {
        if self.status.as_deref() == Some("failed") {
            let code = self
                .error
                .as_ref()
                .and_then(|e| e.code.as_deref())
                .unwrap_or("unknown");
            let msg = self
                .error
                .as_ref()
                .and_then(|e| e.message.as_deref())
                .unwrap_or("no error message");
            anyhow::bail!("OpenAI Responses request failed ({code}): {msg}");
        }
        Ok(())
    }

    fn stop_reason(&self, has_tool_calls: bool) -> Option<crate::providers::traits::StopReason> {
        use crate::providers::traits::StopReason;
        match self.status.as_deref() {
            Some("incomplete") => self
                .incomplete_details
                .as_ref()
                .and_then(|d| d.reason.as_deref())
                .and_then(StopReason::from_wire)
                .or(Some(StopReason::Length)),
            Some("completed") => {
                if has_tool_calls {
                    Some(StopReason::ToolCalls)
                } else {
                    Some(StopReason::Stop)
                }
            }
            _ => None,
        }
    }

    fn collect_text(&self) -> String {
        if let Some(t) = &self.output_text {
            if !t.is_empty() {
                return t.clone();
            }
        }

        let Some(items) = &self.output else {
            return String::new();
        };

        let mut buf = String::new();
        for item in items {

            if let Some(t) = &item.text {
                buf.push_str(t);
                continue;
            }

            let Some(parts) = &item.content else {
                continue;
            };
            for part in parts {
                if let Some(t) = &part.text {
                    if matches!(
                        part.kind.as_deref(),
                        Some("output_text") | Some("text") | None
                    ) {
                        buf.push_str(t);
                    }
                }
            }

            if matches!(item.kind.as_deref(), Some("message")) {
                buf.push('\n');
            }
        }
        buf.trim_end().to_string()
    }

    fn collect_tool_calls(&self) -> Vec<ProviderToolCall> {
        let Some(items) = &self.output else {
            return Vec::new();
        };
        let mut calls = Vec::new();
        for item in items {
            if item.kind.as_deref() != Some("function_call") {
                continue;
            }
            let Some(name) = item.name.as_ref().filter(|n| !n.trim().is_empty()) else {
                continue;
            };
            let id = item
                .call_id
                .clone()
                .or_else(|| item.id.clone())
                .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
            let arguments = crate::providers::sanitize::normalize_tool_call_arguments(
                name,
                item.arguments.clone().unwrap_or_default(),
            );
            calls.push(ProviderToolCall {
                id,
                name: name.clone(),
                arguments,
            });
        }
        calls
    }
}

fn build_responses_input_items(
    messages: &[ChatMessage],
) -> (Option<String>, Vec<serde_json::Value>) {
    let mut instructions: Option<String> = None;
    let mut items: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        match m.role.as_str() {
            "system" => {
                match instructions.as_mut() {
                    Some(existing) => {
                        existing.push_str("\n\n");
                        existing.push_str(&m.content);
                    }
                    None => instructions = Some(m.content.clone()),
                }
            }
            "user" => {
                let (cleaned, images) = crate::multimodal::parse_image_markers(&m.content);
                if images.is_empty() {
                    items.push(serde_json::json!({
                        "type": "message",
                        "role": "user",
                        "content": [{ "type": "input_text", "text": m.content }],
                    }));
                } else {
                    let mut parts = Vec::new();
                    if !cleaned.trim().is_empty() {
                        parts.push(serde_json::json!({ "type": "input_text", "text": cleaned }));
                    }
                    for img in images {
                        parts.push(serde_json::json!({ "type": "input_image", "image_url": img }));
                    }
                    items.push(serde_json::json!({
                        "type": "message",
                        "role": "user",
                        "content": parts,
                    }));
                }
            }
            "assistant" => {
                let envelope =
                    serde_json::from_str::<serde_json::Value>(m.content.trim()).ok();
                let tool_calls = envelope
                    .as_ref()
                    .and_then(|v| v.get("tool_calls"))
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let text = envelope
                    .as_ref()
                    .and_then(|v| v.get("content"))
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| {
                        if tool_calls.is_empty() {
                            m.content.clone()
                        } else {
                            String::new()
                        }
                    });
                if !text.trim().is_empty() {
                    items.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": text }],
                    }));
                }
                for tc in tool_calls {
                    let func = tc.get("function").unwrap_or(&tc);
                    let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.trim().is_empty() {
                        continue;
                    }
                    let id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
                    let args = func
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");
                    items.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": name,
                        "arguments": args,
                    }));
                }
            }
            "tool" => {
                let envelope =
                    serde_json::from_str::<serde_json::Value>(m.content.trim()).ok();
                let call_id = envelope
                    .as_ref()
                    .and_then(|v| {
                        v.get("tool_call_id")
                            .or_else(|| v.get("tool_use_id"))
                            .and_then(|c| c.as_str())
                    })
                    .unwrap_or("")
                    .to_string();
                let output = envelope
                    .as_ref()
                    .and_then(|v| v.get("content"))
                    .map(|c| match c.as_str() {
                        Some(s) => s.to_string(),
                        None => c.to_string(),
                    })
                    .unwrap_or_else(|| m.content.clone());
                if call_id.is_empty() {
                    items.push(serde_json::json!({
                        "type": "message",
                        "role": "user",
                        "content": [{ "type": "input_text", "text": format!("[tool result]\n{output}") }],
                    }));
                } else {
                    items.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": output,
                    }));
                }
            }
            _ => {}
        }
    }
    (instructions, items)
}

fn responses_tools_from_specs(
    tools: Option<&[crate::tools::ToolSpec]>,
) -> Option<Vec<serde_json::Value>> {
    let tools = tools?;
    if tools.is_empty() {
        return None;
    }
    let out: Vec<serde_json::Value> = tools
        .iter()
        .filter(|t| !t.name.trim().is_empty())
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            })
        })
        .collect();
    if out.is_empty() { None } else { Some(out) }
}

fn responses_tools_from_json(tools: &[serde_json::Value]) -> Option<Vec<serde_json::Value>> {
    if tools.is_empty() {
        return None;
    }
    let out: Vec<serde_json::Value> = tools
        .iter()
        .filter_map(|t| {
            let func = t.get("function").unwrap_or(t);
            let name = func.get("name").and_then(|v| v.as_str()).unwrap_or_default();
            if name.trim().is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "type": "function",
                "name": name,
                "description": func.get("description").and_then(|v| v.as_str()).unwrap_or_default(),
                "parameters": func
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} })),
            }))
        })
        .collect();
    if out.is_empty() { None } else { Some(out) }
}

#[async_trait]
impl Provider for OpenAiResponsesProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
            prompt_caching: true,
            responses_api: true,
        }
    }

    fn supports_native_tools(&self) -> bool {
        true
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
                "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml \
                 (provider = openai-responses)."
            )
        })?;

        let mut request = serde_json::json!({
            "model": model,
            "input": message,
        });
        Self::apply_temperature(&mut request, model, temperature);
        self.apply_reasoning(&mut request, model);
        self.apply_prompt_cache_key(&mut request);
        if let Some(sys) = system_prompt {
            request["instructions"] = serde_json::Value::String(sys.to_string());
        }
        if let Some(max) = self.max_output_tokens {
            request["max_output_tokens"] = serde_json::json!(max);
        }

        let response = crate::providers::core::idempotency::apply_idempotency_header(
            self.http_client()
                .post(format!("{}/responses", self.base_url)),
        )
            .header("Authorization", format!("Bearer {credential}"))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI Responses", response).await);
        }

        let payload: ResponsesPayload = response.json().await?;
        payload.ensure_not_failed()?;
        let text = payload.collect_text();
        if text.is_empty() {
            anyhow::bail!("OpenAI Responses returned an empty payload");
        }
        Ok(text)
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let sanitized = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            self.max_output_tokens.unwrap_or(0) as usize,
            None,
        );
        let tools = responses_tools_from_specs(request.tools);
        self.run_tools_request(&sanitized, tools, model, temperature)
            .await
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let sanitized = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            model,
            self.max_output_tokens.unwrap_or(0) as usize,
            None,
        );
        let tools = responses_tools_from_json(tools);
        self.run_tools_request(&sanitized, tools, model, temperature)
            .await
    }

    async fn chat_structured(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<StructuredResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml \
                 (provider = openai-responses)."
            )
        })?;

        let (instructions, input_items) = build_responses_input_items(messages);

        let strict_schema =
            crate::tools::schema::SchemaCleanr::prepare_for_strict_output(schema.clone());
        let mut body = serde_json::json!({
            "model": model,
            "input": input_items,
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "structured_output",
                    "schema": strict_schema,
                    "strict": true
                }
            }
        });
        Self::apply_temperature(&mut body, model, temperature);
        self.apply_reasoning(&mut body, model);
        self.apply_prompt_cache_key(&mut body);
        if let Some(max) = self.max_output_tokens {
            body["max_output_tokens"] = serde_json::json!(max);
        }
        if let Some(instr) = instructions {
            body["instructions"] = serde_json::Value::String(instr);
        }

        let response = crate::providers::core::idempotency::apply_idempotency_header(
            self.http_client()
                .post(format!("{}/responses", self.base_url)),
        )
            .header("Authorization", format!("Bearer {credential}"))
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::super::api_error("OpenAI Responses", response).await);
        }

        let parsed: ResponsesPayload = response.json().await?;
        parsed.ensure_not_failed()?;
        let usage = parsed.usage.as_ref().map(|u| {
            let cached = u
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens);
            crate::observability::subsystem_metrics::observe_prompt_cache_usage(cached, None);
            TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cached_input_tokens: cached,
                cache_creation_input_tokens: None,
                reasoning_tokens: u
                    .output_tokens_details
                    .as_ref()
                    .and_then(|d| d.reasoning_tokens),
            }
        });
        let raw = parsed.collect_text();
        let value = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .or_else(|| parse_first_json_object(&raw))
            .ok_or_else(|| {
                anyhow::anyhow!("OpenAI Responses returned non-JSON payload: {raw}")
            })?;
        Ok(StructuredResponse {
            data: value,
            raw_text: raw,
            usage,
        })
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        if let Some(credential) = self.credential.as_ref() {
            self.http_client()
                .get(format!("{}/models", self.base_url))
                .header("Authorization", format!("Bearer {credential}"))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }
}
