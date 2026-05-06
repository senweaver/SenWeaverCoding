// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Interactive approval workflow for supervised mode.
//!
//! Provides a pre-execution hook that prompts the user before tool calls,
//! with session-scoped "Always" allowlists and audit logging.

use crate::config::AutonomyConfig;
use crate::security::AutonomyLevel;
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::sync::OnceLock;

static PENDING_GATEWAY_APPROVALS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn pending_gateway_approvals() -> &'static Mutex<HashSet<String>> {
    PENDING_GATEWAY_APPROVALS.get_or_init(Default::default)
}

pub fn register_pending_gateway_approval(id: String) {
    pending_gateway_approvals().lock().insert(id);
}

pub fn claim_pending_gateway_approval(id: &str) -> bool {
    pending_gateway_approvals().lock().remove(id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalResponse {

    Yes,

    No,

    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalLogEntry {
    pub timestamp: String,
    pub tool_name: String,
    pub arguments_summary: String,
    pub decision: ApprovalResponse,
    pub channel: String,
}

pub struct ApprovalManager {

    auto_approve: HashSet<String>,

    always_ask: HashSet<String>,

    autonomy_level: AutonomyLevel,

    non_interactive: bool,

    session_allowlist: Mutex<HashSet<String>>,

    audit_log: Mutex<Vec<ApprovalLogEntry>>,

    session_sink: Mutex<Option<crate::session::SessionEventSink>>,
}

impl ApprovalManager {

    pub fn from_config(config: &AutonomyConfig) -> Self {
        Self {
            auto_approve: config.auto_approve.iter().cloned().collect(),
            always_ask: config.always_ask.iter().cloned().collect(),
            autonomy_level: config.level,
            non_interactive: false,
            session_allowlist: Mutex::new(HashSet::new()),
            audit_log: Mutex::new(Vec::new()),
            session_sink: Mutex::new(None),
        }
    }

    pub fn for_non_interactive(config: &AutonomyConfig) -> Self {
        Self {
            auto_approve: config.auto_approve.iter().cloned().collect(),
            always_ask: config.always_ask.iter().cloned().collect(),
            autonomy_level: config.level,
            non_interactive: true,
            session_allowlist: Mutex::new(HashSet::new()),
            audit_log: Mutex::new(Vec::new()),
            session_sink: Mutex::new(None),
        }
    }

    pub fn with_session_sink(self, sink: crate::session::SessionEventSink) -> Self {
        *self.session_sink.lock() = Some(sink);
        self
    }

    pub fn set_session_sink(&self, sink: Option<crate::session::SessionEventSink>) {
        *self.session_sink.lock() = sink;
    }

    pub fn has_session_sink(&self) -> bool {
        self.session_sink.lock().is_some()
    }

    pub fn request_via_session(&self, request: &ApprovalRequest) -> Option<String> {
        let sink = self.session_sink.lock().clone()?;
        let id = format!(
            "appr_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        sink.emit_kind(crate::session::SessionEventKind::ApprovalRequested {
            id: id.clone(),
            tool_name: request.tool_name.clone(),
            arguments: request.arguments.clone(),
            issued_at: Utc::now(),
        });
        register_pending_gateway_approval(id.clone());
        crate::observability::session_write_mode_metrics::incr_approval_routed_via_session();
        Some(id)
    }

    pub async fn poll_response_via_session(
        &self,
        approval_id: &str,
        mut session_rx: tokio::sync::broadcast::Receiver<crate::session::SessionEvent>,
        timeout_ms: u64,
    ) -> ApprovalResponse {
        use crate::session::SessionEventKind;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return ApprovalResponse::No;
            }
            match tokio::time::timeout(remaining, session_rx.recv()).await {
                Ok(Ok(evt)) => {
                    if let SessionEventKind::ApprovalResponded { id, decision, .. } = &evt.kind {
                        if id == approval_id {
                            return match decision.to_ascii_lowercase().as_str() {
                                "yes" | "y" => ApprovalResponse::Yes,
                                "always" | "a" => ApprovalResponse::Always,
                                _ => ApprovalResponse::No,
                            };
                        }
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => {
                    return ApprovalResponse::No;
                }
            }
        }
    }

    pub fn is_non_interactive(&self) -> bool {
        self.non_interactive
    }

    pub fn needs_approval(&self, tool_name: &str) -> bool {

        if self.autonomy_level == AutonomyLevel::Full {
            return false;
        }

        if self.autonomy_level == AutonomyLevel::ReadOnly {
            return false;
        }

        if self.always_ask.contains("*") || self.always_ask.contains(tool_name) {
            return true;
        }

        if self.non_interactive && tool_name == "shell" {
            return false;
        }

        if self.auto_approve.contains("*") || self.auto_approve.contains(tool_name) {
            return false;
        }

        let allowlist = self.session_allowlist.lock();
        if allowlist.contains(tool_name) {
            return false;
        }

        true
    }

    pub fn record_decision(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        decision: ApprovalResponse,
        channel: &str,
    ) {

        if decision == ApprovalResponse::Always {
            let mut allowlist = self.session_allowlist.lock();
            allowlist.insert(tool_name.to_string());
        }

        let summary = summarize_args(args);
        let entry = ApprovalLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            tool_name: tool_name.to_string(),
            arguments_summary: summary,
            decision,
            channel: channel.to_string(),
        };
        let mut log = self.audit_log.lock();
        log.push(entry);
    }

    pub fn audit_log(&self) -> Vec<ApprovalLogEntry> {
        self.audit_log.lock().clone()
    }

    pub fn session_allowlist(&self) -> HashSet<String> {
        self.session_allowlist.lock().clone()
    }

    pub fn prompt_cli(&self, request: &ApprovalRequest) -> ApprovalResponse {
        prompt_cli_interactive(request)
    }
}

fn prompt_cli_interactive(request: &ApprovalRequest) -> ApprovalResponse {
    let summary = summarize_args(&request.arguments);
    eprintln!();
    eprintln!("🔧 Agent wants to execute: {}", request.tool_name);
    eprintln!("   {summary}");
    eprint!("   [Y]es / [N]o / [A]lways for {}: ", request.tool_name);
    let _ = io::stderr().flush();

    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        return ApprovalResponse::No;
    }

    match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => ApprovalResponse::Yes,
        "a" | "always" => ApprovalResponse::Always,
        _ => ApprovalResponse::No,
    }
}

fn summarize_args(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let val = match v {
                        serde_json::Value::String(s) => truncate_for_summary(s, 80),
                        other => {
                            let s = other.to_string();
                            truncate_for_summary(&s, 80)
                        }
                    };
                    format!("{k}: {val}")
                })
                .collect();
            parts.join(", ")
        }
        other => {
            let s = other.to_string();
            truncate_for_summary(&s, 120)
        }
    }
}

fn truncate_for_summary(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}\u{2026}")
    } else {
        input.to_string()
    }
}

