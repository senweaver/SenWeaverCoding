// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::channels::session::backend::SessionBackend;
use crate::providers::traits::ChatMessage;
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;
use std::sync::Arc;

fn resolve_session_messages(
    fallback: Arc<dyn SessionBackend>,
    session_id: &str,
) -> (String, Vec<ChatMessage>) {
    let trimmed = session_id.trim();
    let bare = trimmed
        .strip_prefix("session:")
        .unwrap_or(trimmed)
        .trim()
        .to_string();
    let prefixed = if bare.starts_with("gw_") {
        bare.clone()
    } else {
        format!("gw_{bare}")
    };

    if let Some(global) = crate::channels::session::global_session_backend() {
        for key in [&prefixed, &bare] {
            let msgs = global.load(key);
            if !msgs.is_empty() {
                return (key.clone(), msgs);
            }
        }
    }

    for key in [&bare, &prefixed] {
        let msgs = fallback.load(key);
        if !msgs.is_empty() {
            return (key.clone(), msgs);
        }
    }

    (bare, Vec::new())
}

fn snippet(content: &str, max: usize) -> String {
    let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(max).collect();
    format!("{truncated}…")
}

fn validate_session_id(session_id: &str) -> Result<(), ToolResult> {
    let trimmed = session_id.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_alphanumeric()) {
        return Err(ToolResult {
            success: false,
            output: String::new(),
            error: Some(
                "Invalid 'session_id': must be non-empty and contain at least one alphanumeric character.".into(),
            ),
        });
    }
    Ok(())
}

pub struct SessionsListTool {
    backend: Arc<dyn SessionBackend>,
}

impl SessionsListTool {
    pub fn new(backend: Arc<dyn SessionBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for SessionsListTool {
    fn name(&self) -> &str {
        "sessions_list"
    }

    fn description(&self) -> &str {
        "List all active conversation sessions with their channel, last activity time, and message count."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Max sessions to return (default: 50)"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        #[allow(clippy::cast_possible_truncation)]
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(50, |v| v as usize);

        let backend = self.backend.clone();
        let metadata = tokio::task::spawn_blocking(move || backend.list_sessions_with_metadata())
            .await
            .unwrap_or_default();

        if metadata.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: "No active sessions found.".into(),
                error: None,
            });
        }

        let capped: Vec<_> = metadata.into_iter().take(limit).collect();
        let mut output = format!("Found {} session(s):\n", capped.len());
        for meta in &capped {

            let channel = meta.key.split("__").next().unwrap_or(&meta.key);
            let _ = writeln!(
                output,
                "- {}: channel={}, messages={}, last_activity={}",
                meta.key, channel, meta.message_count, meta.last_activity
            );
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

pub struct SessionsHistoryTool {
    backend: Arc<dyn SessionBackend>,
    security: Arc<SecurityPolicy>,
}

impl SessionsHistoryTool {
    pub fn new(backend: Arc<dyn SessionBackend>, security: Arc<SecurityPolicy>) -> Self {
        Self { backend, security }
    }
}

#[async_trait]
impl Tool for SessionsHistoryTool {
    fn name(&self) -> &str {
        "sessions_history"
    }

    fn description(&self) -> &str {
        "Read the message history of a specific session by its session ID. Returns the last N messages by default, or a bounded slice when 'offset' is provided. Use this together with sessions_search to pull a small contiguous window of context from a referenced session instead of loading the whole conversation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "The session ID to read history from (a bare UUID for a desktop chat, or e.g. telegram__user123 for a channel session)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max messages to return (default: 20)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Zero-based start index into the session's message list. When omitted, the most recent 'limit' messages are returned."
                }
            },
            "required": ["session_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Read, "sessions_history")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'session_id' parameter"))?;

        if let Err(result) = validate_session_id(session_id) {
            return Ok(result);
        }

        #[allow(clippy::cast_possible_truncation)]
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(20, |v| v as usize)
            .max(1);

        #[allow(clippy::cast_possible_truncation)]
        let offset = args
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize);

        let backend = self.backend.clone();
        let session_id_owned = session_id.to_string();
        let (resolved_key, messages) =
            tokio::task::spawn_blocking(move || resolve_session_messages(backend, &session_id_owned))
                .await
                .unwrap_or_else(|_| (session_id.to_string(), Vec::new()));

        if messages.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No messages found for session '{session_id}'."),
                error: None,
            });
        }

        let total = messages.len();
        let (start, slice) = match offset {
            Some(off) => {
                let start = off.min(total);
                let end = start.saturating_add(limit).min(total);
                (start, &messages[start..end])
            }
            None => {
                let start = total.saturating_sub(limit);
                (start, &messages[start..])
            }
        };

        let mut output = format!(
            "Session '{}' ({}): showing messages {}..{} of {}\n",
            session_id,
            resolved_key,
            start,
            start + slice.len(),
            total
        );
        for (i, msg) in slice.iter().enumerate() {
            let _ = writeln!(output, "#{} [{}] {}", start + i, msg.role, msg.content);
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

pub struct SessionsSearchTool {
    backend: Arc<dyn SessionBackend>,
    security: Arc<SecurityPolicy>,
}

impl SessionsSearchTool {
    pub fn new(backend: Arc<dyn SessionBackend>, security: Arc<SecurityPolicy>) -> Self {
        Self { backend, security }
    }
}

#[async_trait]
impl Tool for SessionsSearchTool {
    fn name(&self) -> &str {
        "sessions_search"
    }

    fn description(&self) -> &str {
        "Search the message history of a specific session for a keyword and return matching message snippets (role + index + text). Use this to actively locate relevant context inside a referenced past session instead of loading the entire conversation; then optionally read a small window around a match with sessions_history (offset/limit)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "The session ID to search (a bare UUID for a desktop chat, or e.g. telegram__user123 for a channel session)"
                },
                "keyword": {
                    "type": "string",
                    "description": "Case-insensitive keyword or phrase to find within message contents"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max matching snippets to return (default: 20)"
                }
            },
            "required": ["session_id", "keyword"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Read, "sessions_search")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'session_id' parameter"))?;

        if let Err(result) = validate_session_id(session_id) {
            return Ok(result);
        }

        let keyword = args
            .get("keyword")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'keyword' parameter"))?;

        if keyword.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Search 'keyword' must not be empty.".into()),
            });
        }

        #[allow(clippy::cast_possible_truncation)]
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(20, |v| v as usize)
            .max(1);

        let backend = self.backend.clone();
        let session_id_owned = session_id.to_string();
        let (resolved_key, messages) =
            tokio::task::spawn_blocking(move || resolve_session_messages(backend, &session_id_owned))
                .await
                .unwrap_or_else(|_| (session_id.to_string(), Vec::new()));

        if messages.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No messages found for session '{session_id}'."),
                error: None,
            });
        }

        let needle = keyword.to_lowercase();
        let mut matches: Vec<(usize, &ChatMessage)> = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            if msg.content.to_lowercase().contains(&needle) {
                matches.push((i, msg));
            }
        }

        if matches.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!(
                    "No messages in session '{}' ({}) match '{}'.",
                    session_id, resolved_key, keyword
                ),
                error: None,
            });
        }

        let total_matches = matches.len();
        let shown = matches.into_iter().take(limit).collect::<Vec<_>>();
        let mut output = format!(
            "Session '{}' ({}): {} message(s) match '{}', showing {}:\n",
            session_id,
            resolved_key,
            total_matches,
            keyword,
            shown.len()
        );
        for (i, msg) in shown {
            let _ = writeln!(output, "#{} [{}] {}", i, msg.role, snippet(&msg.content, 240));
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

pub struct SessionsSendTool {
    backend: Arc<dyn SessionBackend>,
    security: Arc<SecurityPolicy>,
}

impl SessionsSendTool {
    pub fn new(backend: Arc<dyn SessionBackend>, security: Arc<SecurityPolicy>) -> Self {
        Self { backend, security }
    }
}

#[async_trait]
impl Tool for SessionsSendTool {
    fn name(&self) -> &str {
        "sessions_send"
    }

    fn description(&self) -> &str {
        "Send a message to a specific session by its session ID. The message is appended to the session's conversation history as a 'user' message, enabling inter-agent communication."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "The target session ID (e.g. telegram__user123)"
                },
                "message": {
                    "type": "string",
                    "description": "The message content to send"
                }
            },
            "required": ["session_id", "message"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "sessions_send")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'session_id' parameter"))?;

        if let Err(result) = validate_session_id(session_id) {
            return Ok(result);
        }

        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'message' parameter"))?;

        if message.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Message content must not be empty.".into()),
            });
        }

        let chat_msg = crate::providers::traits::ChatMessage::user(message);

        let backend = self.backend.clone();
        let session_id_owned = session_id.to_string();
        let session_id_for_msg = session_id.to_string();
        let append_result = tokio::task::spawn_blocking(move || {
            backend.append(&session_id_owned, &chat_msg)
        })
        .await;
        match append_result {
            Ok(Ok(())) => Ok(ToolResult {
                success: true,
                output: format!("Message sent to session '{session_id_for_msg}'."),
                error: None,
            }),
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to send message: {e}")),
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to send message: blocking task join failed: {e}")),
            }),
        }
    }
}
