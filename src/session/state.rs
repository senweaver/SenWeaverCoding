// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::observability::session_write_mode_metrics;
use crate::session::event::{SessionEvent, SessionEventKind};
use crate::session::persistence::SessionEventLog;
use crate::session::sync::SessionSyncHub;

pub type SessionId = String;

pub type AgentId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub id: SessionId,
    pub turns: Vec<Turn>,
    pub edits: Vec<EditBatchRef>,
    pub open_files: HashMap<PathBuf, OpenFileState>,
    pub active_agents: HashMap<AgentId, AgentRuntimeState>,
    pub pending_approvals: Vec<ApprovalRecord>,
    pub metrics: SessionMetrics,

    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub seq: u64,
    pub input: String,

    pub output: Option<String>,
    pub tool_calls: Vec<ToolInvocation>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,

    pub result: Option<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditBatchRef {
    pub id: String,
    pub paths: Vec<PathBuf>,

    pub journal_id: Option<String>,
    pub checkpoint_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenFileState {
    pub last_seen_version: u64,
    pub last_read_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRuntimeState {
    pub last_first_token_ms: Option<u64>,
    pub total_tool_calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub issued_at: DateTime<Utc>,
    pub decision: Option<String>,
    pub responder: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub total_turns: u64,
    pub total_tokens: u64,
    pub total_tool_calls: u64,
    pub total_compressions: u64,
    pub total_diff_sessions_applied: u64,
    pub total_diff_sessions_rolled_back: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDelta {

    pub event: SessionEvent,

    pub version: u64,

    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDelta {

    pub source_session_id: String,

    pub last_seen_seq: u64,

    pub delta: SessionDelta,
}

impl SessionState {
    pub fn new(id: impl Into<SessionId>) -> Self {
        Self {
            id: id.into(),
            turns: Vec::new(),
            edits: Vec::new(),
            open_files: HashMap::new(),
            active_agents: HashMap::new(),
            pending_approvals: Vec::new(),
            metrics: SessionMetrics::default(),
            version: 0,
        }
    }

    pub fn apply(&mut self, evt: &SessionEvent) -> u64 {
        match &evt.kind {
            SessionEventKind::TurnStarted { input } => {
                let seq = self.turns.len() as u64;
                self.turns.push(Turn {
                    seq,
                    input: input.clone(),
                    output: None,
                    tool_calls: Vec::new(),
                    started_at: evt.timestamp,
                    finished_at: None,
                });
                self.metrics.total_turns += 1;
            }
            SessionEventKind::Delta { text } => {
                if let Some(last) = self.turns.last_mut() {
                    match last.output.as_mut() {
                        Some(buf) => buf.push_str(text),
                        None => last.output = Some(text.clone()),
                    }
                }
            }
            SessionEventKind::FirstToken {
                agent_id,
                elapsed_ms,
            } => {
                self.active_agents
                    .entry(agent_id.clone())
                    .or_default()
                    .last_first_token_ms = Some(*elapsed_ms);
            }
            SessionEventKind::ToolCall {
                tool_name,
                tool_call_id,
                arguments,
            } => {
                if let Some(last) = self.turns.last_mut() {
                    last.tool_calls.push(ToolInvocation {
                        call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        arguments: arguments.clone(),
                        result: None,
                        is_error: false,
                    });
                }
                self.metrics.total_tool_calls += 1;
            }
            SessionEventKind::ToolResult {
                tool_call_id,
                output,
                is_error,
            } => {
                if let Some(last) = self.turns.last_mut() {
                    if let Some(inv) = last
                        .tool_calls
                        .iter_mut()
                        .rev()
                        .find(|t| t.call_id == *tool_call_id)
                    {
                        inv.result = Some(output.clone());
                        inv.is_error = *is_error;
                    }
                }
            }
            SessionEventKind::TurnFinished {
                output,
                tokens_used,
            } => {
                if let Some(last) = self.turns.last_mut() {
                    if last.output.is_none() && !output.is_empty() {
                        last.output = Some(output.clone());
                    }
                    last.finished_at = Some(evt.timestamp);
                }
                self.metrics.total_tokens += *tokens_used;
            }
            SessionEventKind::ContextCompressed { .. } => {
                self.metrics.total_compressions += 1;
            }
            SessionEventKind::DiffSessionApplied { .. } => {
                self.metrics.total_diff_sessions_applied += 1;
            }
            SessionEventKind::DiffSessionRolledBack { .. } => {
                self.metrics.total_diff_sessions_rolled_back += 1;
            }
            SessionEventKind::ApprovalRequested {
                id,
                tool_name,
                arguments,
                issued_at,
            } => {
                if !self.pending_approvals.iter().any(|r| r.id == *id) {
                    self.pending_approvals.push(ApprovalRecord {
                        id: id.clone(),
                        tool_name: tool_name.clone(),
                        arguments: arguments.clone(),
                        issued_at: *issued_at,
                        decision: None,
                        responder: None,
                    });
                }
            }
            SessionEventKind::ApprovalResponded {
                id,
                decision,
                responder,
                updated_input: _,
            } => {
                if let Some(rec) = self.pending_approvals.iter_mut().find(|r| r.id == *id) {
                    rec.decision = Some(decision.clone());
                    rec.responder = responder.clone();
                }
            }
            SessionEventKind::CheckpointCreated {
                cp_id,
                edit_batch_id,
            } => {
                if let Some(batch) = self
                    .edits
                    .iter_mut()
                    .find(|b| edit_batch_id.as_deref() == Some(b.id.as_str()))
                {
                    batch.checkpoint_id = Some(cp_id.clone());
                } else {
                    self.edits.push(EditBatchRef {
                        id: edit_batch_id.clone().unwrap_or_else(|| cp_id.clone()),
                        paths: Vec::new(),
                        journal_id: None,
                        checkpoint_id: Some(cp_id.clone()),
                    });
                }
            }
            SessionEventKind::OpenFileMarked {
                path,
                cursor: _,
                source: _,
            } => {

                let key = std::path::PathBuf::from(path);
                let entry = self.open_files.entry(key).or_default();
                entry.last_seen_version = self.version + 1;
                entry.last_read_at = Some(evt.timestamp);
            }
            SessionEventKind::Error { .. }
            | SessionEventKind::ModeChanged { .. }
            | SessionEventKind::WritePlanCreated { .. }
            | SessionEventKind::WriteStepStarted { .. }
            | SessionEventKind::WriteStepFinished { .. }
            | SessionEventKind::WriteVerify { .. }
            | SessionEventKind::ProviderRetry { .. }
            | SessionEventKind::WorkerSpawned { .. }
            | SessionEventKind::WorkerStatus { .. }
            | SessionEventKind::WorkerProgress { .. }
            | SessionEventKind::WorkerCompleted { .. }
            | SessionEventKind::WorkerStopped { .. }
            | SessionEventKind::ParentResumed { .. } => {}
        }
        self.version += 1;
        self.version
    }
}

pub struct SessionActor {
    state: RwLock<SessionState>,
    log: Arc<SessionEventLog>,
    hub: Arc<SessionSyncHub>,

    conflict_count: AtomicU64,
}

impl SessionActor {

    pub fn new(
        id: impl Into<SessionId>,
        log: Arc<SessionEventLog>,
        hub: Arc<SessionSyncHub>,
    ) -> Arc<Self> {
        let id: SessionId = id.into();
        let state = SessionState::new(id.clone());
        hub.register(id.clone());
        Arc::new(Self {
            state: RwLock::new(state),
            log,
            hub,
            conflict_count: AtomicU64::new(0),
        })
    }

    pub fn open_or_create(
        id: impl Into<SessionId>,
        log: Arc<SessionEventLog>,
        hub: Arc<SessionSyncHub>,
    ) -> Arc<Self> {
        let id: SessionId = id.into();
        let replayed = log.replay(&id).unwrap_or_else(|err| {
            tracing::warn!(
                session_id = %id,
                error = %err,
                "session log replay failed; starting from empty state"
            );
            None
        });

        let state = match replayed {
            Some(state) => {
                session_write_mode_metrics::incr_session_replayed();
                state
            }
            None => SessionState::new(id.clone()),
        };

        hub.register(id.clone());
        Arc::new(Self {
            state: RwLock::new(state),
            log,
            hub,
            conflict_count: AtomicU64::new(0),
        })
    }

    pub fn id(&self) -> SessionId {
        self.state.read().id.clone()
    }

    pub fn snapshot(&self) -> SessionState {
        self.state.read().clone()
    }

    pub fn turns_since(&self, since: u64) -> Vec<Turn> {
        self.state
            .read()
            .turns
            .iter()
            .filter(|t| t.seq > since)
            .cloned()
            .collect()
    }

    pub fn apply(&self, evt: &SessionEvent) -> SessionDelta {
        let version;
        {
            let mut guard = self.state.write();
            version = guard.apply(evt);
        }

        let seq = match self.log.append(evt) {
            Ok(seq) => {
                session_write_mode_metrics::incr_session_event_persisted();
                seq
            }
            Err(err) => {
                session_write_mode_metrics::incr_session_apply_failed();
                tracing::warn!(
                    error = %err,
                    "failed to append session event to log; broadcast continues"
                );
                version
            }
        };

        if self.log.needs_snapshot() {
            let guard = self.state.read();
            if let Err(err) = self.log.write_snapshot(&guard) {
                session_write_mode_metrics::incr_session_apply_failed();
                tracing::warn!(
                    error = %err,
                    "failed to write session snapshot"
                );
            }
        }

        let session_id = self.state.read().id.clone();
        let delta = SessionDelta {
            event: evt.clone(),
            version,
            seq,
        };
        self.hub.publish(&session_id, delta.clone());
        delta
    }

    pub fn flush(&self) -> std::io::Result<()> {
        let guard = self.state.read();
        self.log.write_snapshot(&guard)
    }

    pub fn flush_suggested(&self) -> bool {
        self.log.needs_snapshot()
    }

    pub fn apply_remote(&self, remote: RemoteDelta) -> SessionDelta {
        let local_version = self.state.read().version;
        if remote.last_seen_seq < local_version {
            let conflicts = self.conflict_count.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::warn!(
                target: "session.rpc",
                source = %remote.source_session_id,
                remote_last_seen_seq = remote.last_seen_seq,
                local_version = local_version,
                total_conflicts = conflicts,
                "cross-process delta conflict detected; applying with last-writer-wins"
            );
            session_write_mode_metrics::incr_session_rpc_conflict_resolved();
        }
        session_write_mode_metrics::incr_session_rpc_recv();
        self.apply(&remote.delta.event)
    }

    pub fn conflict_count(&self) -> u64 {
        self.conflict_count.load(Ordering::Relaxed)
    }
}

impl Drop for SessionActor {
    fn drop(&mut self) {
        let state = self.state.read();
        if let Err(err) = self.log.write_snapshot(&state) {
            tracing::warn!(
                session_id = %state.id,
                error = %err,
                "final session snapshot failed during drop"
            );
        }
        self.hub.deregister(&state.id);
    }
}

