// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::providers::{ChatMessage, ChatResponse, ConversationMessage, ToolResultMessage};
use crate::tools::{Tool, ToolSpec};
use serde_json::Value;
use std::fmt::Write;

#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub name: String,
    pub arguments: Value,
    pub tool_call_id: Option<String>,

    pub parse_error: bool,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub name: String,
    pub output: String,
    pub success: bool,
    pub tool_call_id: Option<String>,
}

pub trait ToolDispatcher: Send + Sync {
    fn parse_response(&self, response: &ChatResponse) -> (String, Vec<ParsedToolCall>);
    fn format_results(&self, results: &[ToolExecutionResult]) -> ConversationMessage;
    fn prompt_instructions(&self, tools: &[Box<dyn Tool>]) -> String;
    fn to_provider_messages(&self, history: &[ConversationMessage]) -> Vec<ChatMessage>;
    fn should_send_tool_specs(&self) -> bool;
}

#[derive(Default)]
pub struct XmlToolDispatcher;

impl XmlToolDispatcher {
    fn parse_xml_tool_calls(response: &str) -> (String, Vec<ParsedToolCall>) {

        let cleaned = Self::strip_think_tags(response);
        let mut text_parts = Vec::new();
        let mut calls = Vec::new();
        let mut remaining = cleaned.as_str();

        while let Some(start) = remaining.find("<tool_call>") {
            let before = &remaining[..start];
            if !before.trim().is_empty() {
                text_parts.push(before.trim().to_string());
            }

            if let Some(end) = remaining[start..].find("</tool_call>") {
                let inner = &remaining[start + 11..start + end];
                match serde_json::from_str::<Value>(inner.trim()) {
                    Ok(parsed) => {
                        let name = parsed
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if name.is_empty() {
                            remaining = &remaining[start + end + 12..];
                            continue;
                        }
                        let arguments = parsed.get("arguments").cloned().unwrap_or_else(|| {
                            tracing::warn!(
                                tool = %name,
                                "XML tool call missing 'arguments' key; defaulting to empty object"
                            );
                            Value::Object(serde_json::Map::new())
                        });
                        calls.push(ParsedToolCall {
                            name,
                            arguments,
                            tool_call_id: None,
                            parse_error: false,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Malformed <tool_call> JSON: {e}");
                    }
                }
                remaining = &remaining[start + end + 12..];
            } else {
                break;
            }
        }

        if !remaining.trim().is_empty() {
            text_parts.push(remaining.trim().to_string());
        }

        (text_parts.join("\n"), calls)
    }

    fn strip_think_tags(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut rest = s;
        loop {
            if let Some(start) = rest.find("<think>") {
                result.push_str(&rest[..start]);
                if let Some(end) = rest[start..].find("</think>") {
                    rest = &rest[start + end + "</think>".len()..];
                } else {
                    break;
                }
            } else {
                result.push_str(rest);
                break;
            }
        }
        result
    }

    pub fn tool_specs(tools: &[Box<dyn Tool>]) -> Vec<ToolSpec> {
        tools.iter().map(|tool| tool.spec()).collect()
    }
}

impl ToolDispatcher for XmlToolDispatcher {
    fn parse_response(&self, response: &ChatResponse) -> (String, Vec<ParsedToolCall>) {
        let text = response.text_or_empty();
        Self::parse_xml_tool_calls(text)
    }

    fn format_results(&self, results: &[ToolExecutionResult]) -> ConversationMessage {
        let mut content = String::new();
        for result in results {
            let status = if result.success { "ok" } else { "error" };
            let escaped_output = xml_escape(&result.output);
            let _ = writeln!(
                content,
                "<tool_result name=\"{}\" status=\"{}\">\n{}\n</tool_result>",
                result.name, status, escaped_output
            );
        }
        ConversationMessage::Chat(ChatMessage::user(format!("[Tool results]\n{content}")))
    }

    fn prompt_instructions(&self, _tools: &[Box<dyn Tool>]) -> String {
        let mut instructions = String::new();
        instructions.push_str("## Tool Use Protocol\n\n");
        instructions
            .push_str("To use a tool, wrap a JSON object in <tool_call></tool_call> tags:\n\n");
        instructions.push_str(
            "```\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n</tool_call>\n```\n\n",
        );

        instructions
    }

    fn to_provider_messages(&self, history: &[ConversationMessage]) -> Vec<ChatMessage> {
        history
            .iter()
            .flat_map(|msg| match msg {
                ConversationMessage::Chat(chat) => vec![chat.clone()],
                ConversationMessage::AssistantToolCalls { text, .. } => {
                    vec![ChatMessage::assistant(text.clone().unwrap_or_default())]
                }
                ConversationMessage::ToolResults(results) => {
                    let mut content = String::new();
                    for result in results {
                        let _ = writeln!(
                            content,
                            "<tool_result id=\"{}\">\n{}\n</tool_result>",
                            result.tool_call_id, result.content
                        );
                    }
                    vec![ChatMessage::user(format!("[Tool results]\n{content}"))]
                }
            })
            .collect()
    }

    fn should_send_tool_specs(&self) -> bool {
        false
    }
}

fn xml_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '&' => result.push_str("&amp;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            _ => result.push(c),
        }
    }
    result
}

pub struct NativeToolDispatcher;

impl ToolDispatcher for NativeToolDispatcher {
    fn parse_response(&self, response: &ChatResponse) -> (String, Vec<ParsedToolCall>) {
        let text = response.text.clone().unwrap_or_default();
        let calls = response
            .tool_calls
            .iter()
            .map(|tc| ParsedToolCall {
                name: tc.name.clone(),
                arguments: serde_json::from_str(&tc.arguments).unwrap_or_else(|e| {
                    tracing::warn!(
                        tool = %tc.name,
                        error = %e,
                        "Failed to parse native tool call arguments as JSON"
                    );
                    Value::Object(serde_json::Map::new())
                }),
                tool_call_id: Some(tc.id.clone()),
                parse_error: serde_json::from_str::<Value>(&tc.arguments).is_err(),
            })
            .collect();
        (text, calls)
    }

    fn format_results(&self, results: &[ToolExecutionResult]) -> ConversationMessage {
        let messages = results
            .iter()
            .map(|result| ToolResultMessage {
                tool_call_id: result
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                content: result.output.clone(),
            })
            .collect();
        ConversationMessage::ToolResults(messages)
    }

    fn prompt_instructions(&self, _tools: &[Box<dyn Tool>]) -> String {
        String::new()
    }

    fn to_provider_messages(&self, history: &[ConversationMessage]) -> Vec<ChatMessage> {
        history
            .iter()
            .flat_map(|msg| match msg {
                ConversationMessage::Chat(chat) => vec![chat.clone()],
                ConversationMessage::AssistantToolCalls {
                    text,
                    tool_calls,
                    reasoning_content,
                } => {
                    if tool_calls.is_empty() {
                        let mut map = serde_json::Map::new();
                        if let Some(t) = text.as_ref().filter(|s| !s.is_empty()) {
                            map.insert(
                                "content".to_string(),
                                serde_json::Value::String(t.clone()),
                            );
                        }
                        if let Some(rc) =
                            reasoning_content.as_ref().filter(|s| !s.is_empty())
                        {
                            map.insert(
                                "reasoning_content".to_string(),
                                serde_json::Value::String(rc.clone()),
                            );
                        }
                        let body = if map.is_empty() {
                            text.clone().unwrap_or_default()
                        } else {
                            serde_json::Value::Object(map).to_string()
                        };
                        return vec![ChatMessage::assistant(body)];
                    }

                    let content_value = match text {
                        Some(t) if !t.is_empty() => serde_json::Value::String(t.clone()),
                        _ => serde_json::Value::Null,
                    };
                    let mut payload = serde_json::json!({
                        "content": content_value,
                        "tool_calls": tool_calls,
                    });
                    if let Some(rc) = reasoning_content.as_ref().filter(|s| !s.is_empty()) {
                        payload["reasoning_content"] = serde_json::Value::String(rc.clone());
                    }
                    crate::providers::sanitize::ensure_tool_call_id_pair_in_assistant_envelope(
                        &mut payload,
                    );
                    vec![ChatMessage::assistant(payload.to_string())]
                }
                ConversationMessage::ToolResults(results) => results
                    .iter()
                    .map(|result| {
                        let mut envelope = serde_json::json!({
                            "tool_call_id": result.tool_call_id,
                            "tool_use_id": result.tool_call_id,
                            "content": result.content,
                        });
                        crate::providers::sanitize::ensure_tool_id_pair(&mut envelope);
                        ChatMessage::tool(envelope.to_string())
                    })
                    .collect(),
            })
            .collect()
    }

    fn should_send_tool_specs(&self) -> bool {
        true
    }
}
