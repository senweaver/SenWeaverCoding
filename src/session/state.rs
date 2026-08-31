// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

const CONFLICT_JOURNAL_CAP: usize = 256;

const REPLAY_BUFFER_CAP: usize = 1024;

const MAX_RETAINED_TURNS: usize = 512;

const MAX_TURN_TEXT_BYTES: usize = 2 * 1024 * 1024;

const TURN_TEXT_KEEP_BYTES: usize = 1_536 * 1024;

fn cap_turn_text(buf: &mut String) {
    if buf.len() > MAX_TURN_TEXT_BYTES {
        let cut = crate::util::ceil_char_boundary(buf, buf.len() - TURN_TEXT_KEEP_BYTES);
        buf.replace_range(..cut, "…[truncated]");
    }
}

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
    #[serde(default)]
    pub dropped_turns: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
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
            dropped_turns: 0,
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
                let seq = self.dropped_turns + self.turns.len() as u64;
                self.turns.push(Turn {
                    seq,
                    input: input.clone(),
                    output: None,
                    thinking: None,
                    tool_calls: Vec::new(),
                    started_at: evt.timestamp,
                    finished_at: None,
                });
                if self.turns.len() > MAX_RETAINED_TURNS {
                    let drop_n = self.turns.len() - MAX_RETAINED_TURNS;
                    self.dropped_turns += drop_n as u64;
                    self.turns.drain(..drop_n);
                }
                self.metrics.total_turns += 1;
            }
            SessionEventKind::Delta { text } => {
                if let Some(last) = self.turns.last_mut() {
                    match last.output.as_mut() {
                        Some(buf) => {
                            buf.push_str(text);
                            cap_turn_text(buf);
                        }
                        None => last.output = Some(text.clone()),
                    }
                }
            }
            SessionEventKind::Thinking { text } => {
                if let Some(last) = self.turns.last_mut() {
                    match last.thinking.as_mut() {
                        Some(buf) => {
                            buf.push_str(text);
                            cap_turn_text(buf);
                        }
                        None => last.thinking = Some(text.clone()),
                    }
                }
            }
            SessionEventKind::StreamReset => {
                if let Some(last) = self.turns.last_mut() {
                    if last.finished_at.is_none() {
                        last.output = None;
                        last.thinking = None;
                    }
                }
            }
            SessionEventKind::FileEdit {
                path,
                additions: _,
                deletions: _,
            } => {
                let path_buf = std::path::PathBuf::from(path);
                match self.edits.last_mut() {
                    Some(batch) if batch.journal_id.is_none() && batch.checkpoint_id.is_none() => {
                        if !batch.paths.contains(&path_buf) {
                            batch.paths.push(path_buf);
                        }
                    }
                    _ => {
                        let turn_seq = self.dropped_turns + self.turns.len() as u64;
                        self.edits.push(EditBatchRef {
                            id: format!("edits-turn-{turn_seq}-v{}", self.version + 1),
                            paths: vec![path_buf],
                            journal_id: None,
                            checkpoint_id: None,
                        });
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
                    if !output.is_empty() {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConflictRecord {
    pub source_session_id: String,
    pub remote_seq: u64,
    pub remote_version: u64,
    pub remote_last_seen_seq: u64,
    pub local_version: u64,
    pub event_kind: String,
    pub detected_at: DateTime<Utc>,
}

static SESSION_ACTOR_REGISTRY: Lazy<RwLock<HashMap<SessionId, Weak<SessionActor>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn register_session_actor(id: &SessionId, actor: &Arc<SessionActor>) {
    SESSION_ACTOR_REGISTRY
        .write()
        .insert(id.clone(), Arc::downgrade(actor));
}

fn deregister_session_actor(id: &str) {
    SESSION_ACTOR_REGISTRY.write().remove(id);
}

pub fn session_actor(id: &str) -> Option<Arc<SessionActor>> {
    SESSION_ACTOR_REGISTRY.read().get(id).and_then(Weak::upgrade)
}

pub struct SessionActor {
    state: RwLock<SessionState>,
    log: Arc<SessionEventLog>,
    hub: Arc<SessionSyncHub>,

    conflict_count: AtomicU64,

    conflict_journal: Mutex<Vec<SessionConflictRecord>>,

    append_degraded: AtomicBool,

    replay_readonly: AtomicBool,

    pending_replay: Mutex<std::collections::VecDeque<SessionEvent>>,

    replay_dropped: AtomicU64,

    remote_versions: Mutex<HashMap<String, u64>>,

    apply_serialize: Mutex<()>,
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
        let actor = Arc::new(Self {
            state: RwLock::new(state),
            log,
            hub,
            conflict_count: AtomicU64::new(0),
            conflict_journal: Mutex::new(Vec::new()),
            append_degraded: AtomicBool::new(false),
            replay_readonly: AtomicBool::new(false),
            pending_replay: Mutex::new(std::collections::VecDeque::new()),
            replay_dropped: AtomicU64::new(0),
            remote_versions: Mutex::new(HashMap::new()),
            apply_serialize: Mutex::new(()),
        });
        register_session_actor(&id, &actor);
        actor
    }

    pub fn open_or_create(
        id: impl Into<SessionId>,
        log: Arc<SessionEventLog>,
        hub: Arc<SessionSyncHub>,
    ) -> Arc<Self> {
        let id: SessionId = id.into();
        let mut replay_degraded = false;
        let state = match log.replay(&id) {
            Ok(Some(state)) => {
                session_write_mode_metrics::incr_session_replayed();
                state
            }
            Ok(None) => SessionState::new(id.clone()),
            Err(err) => {
                replay_degraded = true;
                tracing::error!(
                    session_id = %id,
                    error = %err,
                    "session log replay failed with an I/O error; on-disk history may exist but could not be read. Entering degraded mode (writes suppressed) to avoid clobbering existing data"
                );
                SessionState::new(id.clone())
            }
        };

        hub.register(id.clone());
        let actor = Arc::new(Self {
            state: RwLock::new(state),
            log,
            hub,
            conflict_count: AtomicU64::new(0),
            conflict_journal: Mutex::new(Vec::new()),
            append_degraded: AtomicBool::new(replay_degraded),
            replay_readonly: AtomicBool::new(replay_degraded),
            pending_replay: Mutex::new(std::collections::VecDeque::new()),
            replay_dropped: AtomicU64::new(0),
            remote_versions: Mutex::new(HashMap::new()),
            apply_serialize: Mutex::new(()),
        });
        register_session_actor(&id, &actor);
        if actor.persistence_degraded() {
            tracing::warn!(
                session_id = %id,
                read_only = actor.log.is_read_only(),
                writer_degraded = actor.log.is_degraded(),
                "session opened with degraded persistence; events may not be durably written to disk"
            );
        }
        actor
    }

    pub fn id(&self) -> SessionId {
        self.state.read().id.clone()
    }

    pub fn snapshot(&self) -> SessionState {
        self.state.read().clone()
    }

    pub fn version(&self) -> u64 {
        self.state.read().version
    }

    pub fn turn_count(&self) -> usize {
        self.state.read().turns.len()
    }

    pub fn last_turn_seq(&self) -> u64 {
        self.state
            .read()
            .turns
            .last()
            .map(|t| t.seq)
            .unwrap_or(0)
    }

    pub fn open_files(&self) -> Vec<(PathBuf, Option<DateTime<Utc>>)> {
        self.state
            .read()
            .open_files
            .iter()
            .map(|(path, meta)| (path.clone(), meta.last_read_at))
            .collect()
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
        self.apply_event(evt, true)
    }

    fn apply_event(&self, evt: &SessionEvent, forward_transport: bool) -> SessionDelta {
        let _apply_guard = self.apply_serialize.lock();
        let version;
        {
            let mut guard = self.state.write();
            version = guard.apply(evt);
        }

        let read_only = self.replay_readonly.load(Ordering::Relaxed);

        let seq = if read_only {
            version
        } else if !self.drain_pending_replay() {
            session_write_mode_metrics::incr_session_apply_failed();
            self.buffer_unpersisted_event(evt);
            version
        } else {
            match self.log.append(evt) {
                Ok(seq) => {
                    session_write_mode_metrics::incr_session_event_persisted();
                    if self.append_degraded.swap(false, Ordering::Relaxed) {
                        tracing::info!(
                            "session persistence recovered; event appends succeeding again"
                        );
                    }
                    seq
                }
                Err(err) => {
                    session_write_mode_metrics::incr_session_apply_failed();
                    self.buffer_unpersisted_event(evt);
                    if self.append_degraded.swap(true, Ordering::Relaxed) {
                        tracing::warn!(
                            error = %err,
                            "session event append still failing in degraded mode; broadcast continues"
                        );
                    } else {
                        tracing::error!(
                            error = %err,
                            "failed to append session event to log; session persistence degraded (events buffered for replay, in-memory state and broadcast continue)"
                        );
                    }
                    version
                }
            }
        };

        if !read_only && self.log.needs_snapshot() {
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
        if forward_transport {
            self.hub.publish(&session_id, delta.clone());
        } else {
            self.hub.publish_local(&session_id, &delta);
        }
        delta
    }

    fn buffer_unpersisted_event(&self, evt: &SessionEvent) {
        let mut buf = self.pending_replay.lock();
        if buf.len() >= REPLAY_BUFFER_CAP {
            buf.pop_front();
            let dropped = self.replay_dropped.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 || dropped % 100 == 0 {
                tracing::warn!(
                    dropped,
                    "session replay buffer full; oldest unpersisted events are being dropped"
                );
            }
        }
        buf.push_back(evt.clone());
    }

    fn drain_pending_replay(&self) -> bool {
        let mut buf = self.pending_replay.lock();
        if buf.is_empty() {
            return true;
        }
        let mut replayed = 0usize;
        while let Some(evt) = buf.front() {
            match self.log.append(evt) {
                Ok(_) => {
                    session_write_mode_metrics::incr_session_event_persisted();
                    buf.pop_front();
                    replayed += 1;
                }
                Err(_) => {
                    if replayed > 0 {
                        tracing::info!(
                            replayed,
                            remaining = buf.len(),
                            "partially replayed buffered session events before writer stalled again"
                        );
                    }
                    return false;
                }
            }
        }
        if replayed > 0 {
            tracing::info!(
                replayed,
                "replayed buffered session events after persistence recovered"
            );
        }
        true
    }

    pub fn flush(&self) -> std::io::Result<()> {
        if self.replay_readonly.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "session replay failed during open; persistence is read-only to avoid clobbering on-disk history",
            ));
        }
        self.drain_pending_replay();
        let guard = self.state.read();
        self.log.write_snapshot(&guard)
    }

    pub fn flush_suggested(&self) -> bool {
        self.log.needs_snapshot()
    }

    pub fn apply_remote(&self, remote: RemoteDelta) -> SessionDelta {
        session_write_mode_metrics::incr_session_rpc_recv();
        if remote.source_session_id == crate::session::rpc::process_instance_id() {
            tracing::debug!(
                target: "session.rpc",
                "ignoring self-originated remote delta (loopback)"
            );
            return remote.delta;
        }
        {
            let mut seen = self.remote_versions.lock();
            let last_applied = seen
                .get(&remote.source_session_id)
                .copied()
                .unwrap_or(0);
            if remote.delta.version <= last_applied {
                tracing::debug!(
                    target: "session.rpc",
                    source = %remote.source_session_id,
                    remote_version = remote.delta.version,
                    last_applied_version = last_applied,
                    "dropping duplicate or stale remote delta; already merged into local state"
                );
                return remote.delta.clone();
            }
            seen.insert(remote.source_session_id.clone(), remote.delta.version);
        }
        let local_version = self.state.read().version;
        if remote.last_seen_seq < local_version {
            let conflicts = self.conflict_count.fetch_add(1, Ordering::Relaxed) + 1;
            let event_kind = serde_json::to_value(&remote.delta.event.kind)
                .ok()
                .and_then(|v| {
                    v.get("type")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "unknown".to_string());
            let record = SessionConflictRecord {
                source_session_id: remote.source_session_id.clone(),
                remote_seq: remote.delta.seq,
                remote_version: remote.delta.version,
                remote_last_seen_seq: remote.last_seen_seq,
                local_version,
                event_kind: event_kind.clone(),
                detected_at: Utc::now(),
            };
            {
                let mut journal = self.conflict_journal.lock();
                if journal.len() >= CONFLICT_JOURNAL_CAP {
                    let overflow = journal.len() + 1 - CONFLICT_JOURNAL_CAP;
                    journal.drain(0..overflow);
                }
                journal.push(record);
            }
            tracing::warn!(
                target: "session.rpc",
                source = %remote.source_session_id,
                remote_last_seen_seq = remote.last_seen_seq,
                remote_seq = remote.delta.seq,
                remote_version = remote.delta.version,
                local_version = local_version,
                event_kind = %event_kind,
                total_conflicts = conflicts,
                "cross-process delta conflict detected; preserving local state and journaling the remote delta before non-destructive merge"
            );
            session_write_mode_metrics::incr_session_rpc_conflict_resolved();
        }
        self.apply_event(&remote.delta.event, false)
    }

    pub fn conflict_count(&self) -> u64 {
        self.conflict_count.load(Ordering::Relaxed)
    }

    pub fn conflict_journal(&self) -> Vec<SessionConflictRecord> {
        self.conflict_journal.lock().clone()
    }

    pub fn persistence_degraded(&self) -> bool {
        self.append_degraded.load(Ordering::Relaxed)
            || self.log.is_read_only()
            || self.log.is_degraded()
    }
}

impl Drop for SessionActor {
    fn drop(&mut self) {
        let state = self.state.read();
        if self.replay_readonly.load(Ordering::Relaxed) {
            tracing::warn!(
                session_id = %state.id,
                "skip final snapshot: session in replay-readonly mode to avoid overwriting on-disk history"
            );
        } else if !self.drain_pending_replay() {
            tracing::warn!(
                session_id = %state.id,
                remaining = self.pending_replay.lock().len(),
                "unpersisted session events could not be replayed before drop"
            );
            if let Err(err) = self.log.write_snapshot(&state) {
                tracing::warn!(
                    session_id = %state.id,
                    error = %err,
                    "final session snapshot failed during drop"
                );
            }
        } else if let Err(err) = self.log.write_snapshot(&state) {
            tracing::warn!(
                session_id = %state.id,
                error = %err,
                "final session snapshot failed during drop"
            );
        }
        self.hub.deregister(&state.id);
        deregister_session_actor(&state.id);
    }
}

