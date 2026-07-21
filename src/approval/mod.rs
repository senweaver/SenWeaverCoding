// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::AutonomyConfig;
use crate::security::AutonomyLevel;
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub const SESSION_APPROVAL_TIMEOUT_MS: u64 = 300_000;

const PENDING_APPROVAL_TTL_SECS: u64 = 30 * 60;

const PENDING_APPROVAL_SWEEP_INTERVAL_SECS: u64 = 5 * 60;

const PENDING_APPROVAL_MAX_ENTRIES: usize = 1_000;

struct PendingGatewayEntry {
    created: Instant,
    session_id: Option<String>,
    replay: Option<serde_json::Value>,
}

struct PendingGatewayApprovals {
    entries: HashMap<String, PendingGatewayEntry>,
    last_sweep: Instant,
}

impl PendingGatewayApprovals {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            last_sweep: Instant::now(),
        }
    }

    fn sweep_locked(&mut self, now: Instant) {
        let ttl = Duration::from_secs(PENDING_APPROVAL_TTL_SECS);
        self.entries
            .retain(|_, entry| now.duration_since(entry.created) < ttl);
        self.last_sweep = now;
    }

    fn maybe_sweep(&mut self, now: Instant) {
        if now.duration_since(self.last_sweep).as_secs() >= PENDING_APPROVAL_SWEEP_INTERVAL_SECS
            || self.entries.len() >= PENDING_APPROVAL_MAX_ENTRIES
        {
            self.sweep_locked(now);
        }
    }

    fn insert(&mut self, id: String, session_id: Option<String>, replay: Option<serde_json::Value>) {
        let now = Instant::now();
        self.maybe_sweep(now);

        if self.entries.len() >= PENDING_APPROVAL_MAX_ENTRIES {
            self.sweep_locked(now);
            if self.entries.len() >= PENDING_APPROVAL_MAX_ENTRIES {
                if let Some(lru) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.created)
                    .map(|(k, _)| k.clone())
                {
                    self.entries.remove(&lru);
                    tracing::warn!(
                        "pending_gateway_approvals at capacity; evicted oldest pending entry"
                    );
                }
            }
        }

        self.entries.insert(
            id,
            PendingGatewayEntry {
                created: now,
                session_id,
                replay,
            },
        );
    }

    fn claim(&mut self, id: &str, session_id: Option<&str>) -> bool {
        let now = Instant::now();
        self.maybe_sweep(now);
        match self.entries.remove(id) {
            Some(entry) if now.duration_since(entry.created).as_secs() < PENDING_APPROVAL_TTL_SECS => {
                match (entry.session_id.as_deref(), session_id) {
                    (Some(expected), Some(actual)) => expected == actual,
                    (Some(_), None) => false,
                    // Registered without a session (e.g. CLI single-turn): only a
                    // responder that also carries no session may claim it. A
                    // responder presenting a concrete session id is a mismatch and
                    // is rejected so it cannot claim another context's approval.
                    (None, None) => true,
                    (None, Some(_)) => false,
                }
            }
            Some(_) => false,
            None => false,
        }
    }

    fn drop_entry(&mut self, id: &str) -> bool {
        let now = Instant::now();
        self.maybe_sweep(now);
        self.entries.remove(id).is_some()
    }
}

static PENDING_GATEWAY_APPROVALS: OnceLock<Mutex<PendingGatewayApprovals>> = OnceLock::new();

fn pending_gateway_approvals() -> &'static Mutex<PendingGatewayApprovals> {
    PENDING_GATEWAY_APPROVALS.get_or_init(|| Mutex::new(PendingGatewayApprovals::new()))
}

fn approval_session_id() -> Option<String> {
    crate::session::current_session_context().map(|ctx| ctx.session_id.clone())
}

pub fn register_pending_gateway_approval(id: String) {
    pending_gateway_approvals()
        .lock()
        .insert(id, approval_session_id(), None);
}

pub fn register_pending_gateway_approval_for_session(id: String, session_id: Option<String>) {
    pending_gateway_approvals().lock().insert(id, session_id, None);
}

/// Register with a client-facing `permission_request` frame that can be
/// replayed to a reconnecting client. Without this, a prompt emitted while the
/// WebSocket was down is simply gone: the reconnected client shows a busy
/// session with no dialog and the waiter blocks until timeout.
pub fn register_pending_gateway_approval_with_replay(
    id: String,
    replay: serde_json::Value,
) {
    pending_gateway_approvals()
        .lock()
        .insert(id, approval_session_id(), Some(replay));
}

/// Replay frames for every approval still outstanding for `session_id`
/// (oldest first). Non-consuming: entries stay pending until claimed/dropped.
pub fn pending_replays_for_session(session_id: &str) -> Vec<serde_json::Value> {
    let guard = pending_gateway_approvals().lock();
    let mut hits: Vec<(&Instant, &serde_json::Value)> = guard
        .entries
        .values()
        .filter(|e| e.session_id.as_deref() == Some(session_id))
        .filter_map(|e| e.replay.as_ref().map(|r| (&e.created, r)))
        .collect();
    hits.sort_by_key(|(created, _)| **created);
    hits.into_iter().map(|(_, r)| r.clone()).collect()
}

pub fn claim_pending_gateway_approval(id: &str) -> bool {
    claim_pending_gateway_approval_for_session(id, approval_session_id().as_deref())
}

pub fn claim_pending_gateway_approval_for_session(id: &str, session_id: Option<&str>) -> bool {
    pending_gateway_approvals()
        .lock()
        .claim(id, session_id)
}

pub fn drop_pending_gateway_approval(id: &str) -> bool {
    pending_gateway_approvals().lock().drop_entry(id)
}

/// Peek the session a pending approval was registered under (outer `None` =
/// unknown id, inner `None` = registered without a session). Lets per-session
/// surfaces forward only their own bus prompts.
pub fn pending_gateway_approval_session(id: &str) -> Option<Option<String>> {
    pending_gateway_approvals()
        .lock()
        .entries
        .get(id)
        .map(|entry| entry.session_id.clone())
}

// Reliable decision mailbox: every emit site records the decision here before
// broadcasting, so a waiter can never lose its verdict to a lagged broadcast
// channel under load (the old failure mode blocked turns for the full 5-minute
// approval timeout).
static DELIVERED_DECISIONS: OnceLock<Mutex<HashMap<String, (ApprovalResponse, Instant)>>> =
    OnceLock::new();

fn delivered_decisions() -> &'static Mutex<HashMap<String, (ApprovalResponse, Instant)>> {
    DELIVERED_DECISIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn record_session_decision_delivery(id: &str, decision: &str) {
    let response = match decision.to_ascii_lowercase().as_str() {
        "yes" | "y" => ApprovalResponse::Yes,
        "always" | "a" => ApprovalResponse::Always,
        _ => ApprovalResponse::No,
    };
    let mut guard = delivered_decisions().lock();
    let now = Instant::now();
    if guard.len() >= PENDING_APPROVAL_MAX_ENTRIES {
        let ttl = Duration::from_secs(PENDING_APPROVAL_TTL_SECS);
        guard.retain(|_, (_, ts)| now.duration_since(*ts) < ttl);
    }
    guard.insert(id.to_string(), (response, now));
}

fn take_delivered_decision(id: &str) -> Option<ApprovalResponse> {
    delivered_decisions().lock().remove(id).map(|(r, _)| r)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionApprovalVerdict {
    Decision(ApprovalResponse),
    Cancelled,
    TimedOut,
}

static SESSION_SURFACE_APPROVAL_MANAGER: OnceLock<ApprovalManager> = OnceLock::new();

pub fn install_session_surface_approval_manager(
    config: &AutonomyConfig,
    audit_log_path: Option<PathBuf>,
) {
    let _ = SESSION_SURFACE_APPROVAL_MANAGER.get_or_init(|| {
        let mgr = ApprovalManager::from_config(config)
            .with_session_sink(crate::gateway::ws::gateway_approval_sink_handle());
        mgr.set_audit_log_path(audit_log_path);
        mgr
    });
}

pub fn session_surface_approval_manager() -> Option<&'static ApprovalManager> {
    SESSION_SURFACE_APPROVAL_MANAGER.get()
}

pub async fn wait_for_session_decision(
    request_id: &str,
    rx: &mut tokio::sync::broadcast::Receiver<crate::session::SessionEvent>,
    cancellation_token: Option<&CancellationToken>,
) -> SessionApprovalVerdict {
    let deadline = tokio::time::Instant::now()
        + Duration::from_millis(SESSION_APPROVAL_TIMEOUT_MS);
    let mut mailbox_poll = tokio::time::interval(Duration::from_millis(250));
    mailbox_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if let Some(response) = take_delivered_decision(request_id) {
            return SessionApprovalVerdict::Decision(response);
        }
        let recv_with_deadline = tokio::time::timeout_at(deadline, rx.recv());
        let received = if let Some(token) = cancellation_token {
            tokio::select! {
                () = token.cancelled() => return SessionApprovalVerdict::Cancelled,
                _ = mailbox_poll.tick() => continue,
                outcome = recv_with_deadline => outcome,
            }
        } else {
            tokio::select! {
                _ = mailbox_poll.tick() => continue,
                outcome = recv_with_deadline => outcome,
            }
        };
        match received {
            Ok(Ok(event)) => {
                if let crate::session::SessionEventKind::ApprovalResponded {
                    id, decision, ..
                } = &event.kind
                {
                    if id == request_id {
                        let response = match decision.to_ascii_lowercase().as_str() {
                            "yes" | "y" => ApprovalResponse::Yes,
                            "always" | "a" => ApprovalResponse::Always,
                            _ => ApprovalResponse::No,
                        };
                        return SessionApprovalVerdict::Decision(response);
                    }
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                if let Some(response) = take_delivered_decision(request_id) {
                    return SessionApprovalVerdict::Decision(response);
                }
                continue;
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => {
                return SessionApprovalVerdict::TimedOut;
            }
        }
    }
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

    allow_shell_in_non_interactive: bool,

    // Keyed by session id so an "Always" grant in one desktop tab never
    // silently auto-approves the same tool in every other parallel session.
    // Contexts without a session (plain CLI) fall back to a shared bucket.
    session_allowlist: Mutex<HashMap<String, HashSet<String>>>,

    audit_log: Mutex<Vec<ApprovalLogEntry>>,

    audit_log_path: Mutex<Option<PathBuf>>,

    session_sink: Mutex<Option<crate::session::SessionEventSink>>,
}

fn allowlist_scope_key() -> String {
    crate::session::current_session_context()
        .map(|c| c.session_id)
        .unwrap_or_else(|| "__no_session__".to_string())
}

impl ApprovalManager {

    pub fn from_config(config: &AutonomyConfig) -> Self {
        Self {
            auto_approve: config.auto_approve.iter().cloned().collect(),
            always_ask: config.always_ask.iter().cloned().collect(),
            autonomy_level: config.level,
            non_interactive: false,
            allow_shell_in_non_interactive: config.allow_shell_in_non_interactive,
            session_allowlist: Mutex::new(HashMap::new()),
            audit_log: Mutex::new(Vec::new()),
            audit_log_path: Mutex::new(None),
            session_sink: Mutex::new(None),
        }
    }

    pub fn for_non_interactive(config: &AutonomyConfig) -> Self {
        if config.allow_shell_in_non_interactive
            && !matches!(
                config.level,
                AutonomyLevel::Full | AutonomyLevel::ReadOnly
            )
            && !config.always_ask.iter().any(|t| t == "shell" || t == "*")
        {
            tracing::warn!(
                "ApprovalManager: non-interactive mode auto-approves the `shell` tool \
                 (autonomy_level={:?}); add `shell` to [autonomy] always_ask or set \
                 [autonomy] allow_shell_in_non_interactive = false to require approval, \
                 or run in interactive mode.",
                config.level
            );
        }
        Self {
            auto_approve: config.auto_approve.iter().cloned().collect(),
            always_ask: config.always_ask.iter().cloned().collect(),
            autonomy_level: config.level,
            non_interactive: true,
            allow_shell_in_non_interactive: config.allow_shell_in_non_interactive,
            session_allowlist: Mutex::new(HashMap::new()),
            audit_log: Mutex::new(Vec::new()),
            audit_log_path: Mutex::new(None),
            session_sink: Mutex::new(None),
        }
    }

    // Single canonical constructor for every surface (interactive REPL,
    // headless one-shot, channel daemons) so the three call sites cannot
    // drift on interactivity semantics or audit-log wiring again.
    pub fn for_surface(
        config: &AutonomyConfig,
        interactive: bool,
        audit_log_path: Option<PathBuf>,
    ) -> Self {
        let mgr = if interactive {
            Self::from_config(config)
        } else {
            Self::for_non_interactive(config)
        };
        mgr.set_audit_log_path(audit_log_path);
        mgr
    }

    pub fn set_audit_log_path(&self, path: Option<PathBuf>) {
        *self.audit_log_path.lock() = path;
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
        let id = format!("appr_{}", uuid::Uuid::new_v4().simple());
        let replay = serde_json::json!({
            "type": "permission_request",
            "requestId": id,
            "toolName": request.tool_name,
            "input": request.arguments,
        });
        pending_gateway_approvals().lock().insert(
            id.clone(),
            approval_session_id(),
            Some(replay),
        );
        sink.emit_kind(crate::session::SessionEventKind::ApprovalRequested {
            id: id.clone(),
            tool_name: request.tool_name.clone(),
            arguments: request.arguments.clone(),
            issued_at: Utc::now(),
        });
        crate::observability::session_write_mode_metrics::incr_approval_routed_via_session();
        Some(id)
    }

    pub fn is_non_interactive(&self) -> bool {
        self.non_interactive
    }

    // Guardrail for coding modes whose policy is AutoApprove (Agent/Harness/
    // Designer): the mode may skip routine confirmations, but it must never
    // override an explicit user `always_ask` entry, and on non-interactive
    // surfaces (channels/daemon/one-shot) it must not bypass the
    // allow_shell_in_non_interactive opt-out for shell execution.
    pub fn mode_auto_approve_allows(&self, tool_name: &str) -> bool {
        if self.autonomy_level == AutonomyLevel::Full {
            return true;
        }
        if self.always_ask.contains("*") || self.always_ask.contains(tool_name) {
            return false;
        }
        if self.non_interactive && tool_name == "shell" && !self.allow_shell_in_non_interactive {
            return false;
        }
        true
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

        if self.non_interactive && tool_name == "shell" && self.allow_shell_in_non_interactive {
            return false;
        }

        if !self.non_interactive && crate::trust::domain_regressed(tool_name) {
            return true;
        }

        if self.auto_approve.contains("*") || self.auto_approve.contains(tool_name) {
            return false;
        }

        let scope = allowlist_scope_key();
        let allowlist = self.session_allowlist.lock();
        if allowlist
            .get(&scope)
            .map(|set| set.contains(tool_name))
            .unwrap_or(false)
        {
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
            let scope = allowlist_scope_key();
            let mut allowlist = self.session_allowlist.lock();
            allowlist
                .entry(scope)
                .or_default()
                .insert(tool_name.to_string());
        }

        let summary = summarize_args(args);
        let entry = ApprovalLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            tool_name: tool_name.to_string(),
            arguments_summary: summary,
            decision,
            channel: channel.to_string(),
        };

        let path = self.audit_log_path.lock().clone();
        if let Some(path) = path {
            if let Err(e) = append_audit_entry_to_disk(&path, &entry) {
                tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "failed to persist approval audit log entry"
                );
            }
        }

        let mut log = self.audit_log.lock();
        log.push(entry);

        let approved = decision != ApprovalResponse::No;
        let reason = if approved {
            ""
        } else {
            "user denied tool execution"
        };
        crate::trust::record_tool_decision(tool_name, approved, reason);
    }

    pub fn audit_log(&self) -> Vec<ApprovalLogEntry> {
        self.audit_log.lock().clone()
    }

    pub fn session_allowlist(&self) -> HashSet<String> {
        let scope = allowlist_scope_key();
        self.session_allowlist
            .lock()
            .get(&scope)
            .cloned()
            .unwrap_or_default()
    }

    pub fn prompt_cli(&self, request: &ApprovalRequest) -> ApprovalResponse {
        prompt_cli_interactive(request)
    }

    pub async fn prompt_cli_async(&self, request: &ApprovalRequest) -> ApprovalResponse {
        let request = request.clone();
        match tokio::task::spawn_blocking(move || prompt_cli_interactive(&request)).await {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!(
                    target: "approval",
                    error = %err,
                    "CLI approval prompt task failed; denying"
                );
                ApprovalResponse::No
            }
        }
    }
}

fn append_audit_entry_to_disk(
    path: &std::path::Path,
    entry: &ApprovalLogEntry,
) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut line = serde_json::to_string(entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

fn prompt_cli_interactive(request: &ApprovalRequest) -> ApprovalResponse {
    use std::io::IsTerminal as _;

    if !io::stdin().is_terminal() {
        tracing::warn!(
            target: "approval",
            tool = %request.tool_name,
            "tool approval requested without an interactive terminal (channel/headless); auto-denying instead of blocking on stdin (configure autonomy auto-approve or use an interactive client to allow)"
        );
        return ApprovalResponse::No;
    }

    let summary = summarize_args(&request.arguments);
    eprintln!();
    eprintln!("🔧 Agent wants to execute: {}", request.tool_name);
    eprintln!("   {summary}");
    eprint!("   [Y]es / [N]o / [A]lways for {}: ", request.tool_name);
    let _ = io::stderr().flush();

    let line = match crate::cli::input::read_line_lossy(&mut io::stdin().lock()) {
        Ok(Some(line)) => line,
        Ok(None) | Err(_) => return ApprovalResponse::No,
    };

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

