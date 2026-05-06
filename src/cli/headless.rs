// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Headless / SDK driver — runs the agent loop without a terminal UI.
//!
//! Mirrors cc-typescript-src's `cli/print.ts`. Processes prompts, executes
//! tools, handles permission prompts via the structured I/O control protocol,
//! manages MCP server lifecycle, and batches tool calls.

use super::structured_io::{ControlResponsePayload, StdinMessage, StdoutMessage, StructuredIO};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResult {
    pub session_id: String,
    pub num_turns: u32,
    pub duration_ms: u64,
    pub cost: f64,
    pub final_response: Option<String>,
    pub exit_reason: ExitReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    EndTurn,
    MaxTurns,
    StopSequence,
    Error,
    UserAbort,
    PermissionDenied,
}

#[derive(Debug, Clone)]
pub struct HeadlessConfig {
    pub session_id: String,
    pub initial_prompt: String,
    pub max_turns: Option<u32>,
    pub model: Option<String>,
    pub system_prompt_append: Option<String>,
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub output_format: OutputFormat,
    pub plan_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Json,
    StreamJson,
    Text,
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            initial_prompt: String::new(),
            max_turns: None,
            model: None,
            system_prompt_append: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            mcp_servers: Vec::new(),
            output_format: OutputFormat::StreamJson,
            plan_mode: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow(Option<serde_json::Value>),
    Deny(Option<String>),
}

impl From<ControlResponsePayload> for PermissionDecision {
    fn from(payload: ControlResponsePayload) -> Self {
        match payload {
            ControlResponsePayload::Allow { updated_input } => {
                PermissionDecision::Allow(updated_input)
            }
            ControlResponsePayload::Deny { reason } => PermissionDecision::Deny(reason),
        }
    }
}

pub async fn run_headless(config: HeadlessConfig, io: &mut StructuredIO) -> Result<SessionResult> {
    let start = Instant::now();
    let mut num_turns: u32 = 0;
    let max_turns = config.max_turns.unwrap_or(u32::MAX);
    let mut final_response: Option<String> = None;
    let mut exit_reason = ExitReason::EndTurn;

    io.notify_session_state(
        &config.session_id,
        "started",
        serde_json::json!({
            "model": config.model,
            "plan_mode": config.plan_mode,
        }),
    );

    tracing::info!(
        session_id = %config.session_id,
        max_turns = ?config.max_turns,
        model = ?config.model,
        plan_mode = config.plan_mode,
        tool_allow_count = config.allowed_tools.len(),
        tool_deny_count = config.denied_tools.len(),
        "Starting headless session"
    );

    if config.output_format == OutputFormat::StreamJson {
        io.emit_system(&format!("Session {} started", config.session_id));
    }

    let loaded_config = Config::load_or_init().await?;

    let allowed_tools = if config.denied_tools.is_empty() {
        if config.allowed_tools.is_empty() {
            None
        } else {
            Some(config.allowed_tools.clone())
        }
    } else {
        let all_tool_names: Vec<String> = vec![
            "shell",
            "file_read",
            "file_write",
            "file_edit",
            "glob_search",
            "grep_search",
            "dir_list",
            "git_operations",
            "browser",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let filtered: Vec<String> = all_tool_names
            .into_iter()
            .filter(|t| !config.denied_tools.contains(t))
            .collect();
        Some(filtered)
    };

    if let Some(bs) = crate::bootstrap::try_get_state() {
        bs.write(|state| {
            if config.allowed_tools.is_empty() && config.denied_tools.is_empty() {
                state.session_bypass_permissions_mode = true;
            }
        });
    }

    let mut current_message: Option<String> = Some(config.initial_prompt.clone());

    loop {
        num_turns += 1;

        if num_turns > max_turns {
            exit_reason = ExitReason::MaxTurns;
            tracing::info!(num_turns, "Max turns reached");
            break;
        }

        match crate::agent::run(
            loaded_config.clone(),
            current_message.take(),
            None,
            config.model.clone(),
            loaded_config.default_temperature,
            Vec::new(),
            false,
            None,
            allowed_tools.clone(),
        )
        .await
        {
            Ok(response) => {
                final_response = Some(response.clone());
                let _ = io.write(&StdoutMessage::AssistantMessage {
                    content: response,
                    stop_reason: Some("end_turn".into()),
                });

                if config.output_format == OutputFormat::StreamJson {
                    if let Some(svc) = crate::services::try_get_services() {
                        if let Ok(summary) = svc.tool_use_summary.lock() {
                            let stats = summary.aggregate();
                            for s in &stats {
                                tracing::info!(
                                    tool = %s.tool_name,
                                    calls = s.call_count,
                                    ok = s.success_count,
                                    fail = s.failure_count,
                                    "headless tool usage"
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => {
                io.emit_system(&format!("Agent error: {e}"));
                exit_reason = ExitReason::Error;
                break;
            }
        }

        match io.recv().await {
            Some(StdinMessage::SdkMessage { action, .. }) if action == "abort" => {
                exit_reason = ExitReason::UserAbort;
                tracing::info!("User abort received");
                break;
            }
            Some(StdinMessage::UserMessage { content, .. }) => {
                current_message = Some(content);
            }
            _ => {
                break;
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    io.notify_session_state(
        &config.session_id,
        "completed",
        serde_json::json!({
            "exit_reason": exit_reason,
            "num_turns": num_turns,
            "duration_ms": duration_ms,
        }),
    );

    let _ = io.write(&StdoutMessage::Result {
        session_id: Some(config.session_id.clone()),
        cost: Some(0.0),
        duration_ms: Some(duration_ms),
        num_turns: Some(num_turns),
    });

    Ok(SessionResult {
        session_id: config.session_id,
        num_turns,
        duration_ms,
        cost: 0.0,
        final_response,
        exit_reason,
    })
}

pub async fn handle_permission_request(
    io: &StructuredIO,
    tool_name: &str,
    input: &serde_json::Value,
    tool_use_id: Option<&str>,
) -> Result<PermissionDecision> {
    let response = io.request_permission(tool_name, input, tool_use_id).await?;
    Ok(PermissionDecision::from(response))
}

pub fn is_tool_allowed(tool_name: &str, allowed: &[String], denied: &[String]) -> Option<bool> {
    if denied.iter().any(|d| d == tool_name || d == "*") {
        return Some(false);
    }
    if !allowed.is_empty() && !allowed.iter().any(|a| a == tool_name || a == "*") {
        return Some(false);
    }
    None
}

pub fn join_prompt_values(values: &[String]) -> String {
    values.join("\n\n")
}
