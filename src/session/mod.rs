// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod bridge;
pub mod chat_view;
pub mod event;

pub mod persistence;
pub mod shell;

pub mod state;

pub mod sync;
pub mod translators;

pub mod rpc;

pub mod os_lock;
pub mod resource_lock;
pub mod run_state;
pub mod turn_feed;
pub mod workspace_run;
pub mod write_lock;

pub use bridge::SessionEventSink;
pub use resource_lock::{
    AcquireError as ResourceAcquireError, ResourceEvent, ResourceGuard, ResourceKind,
    SessionContext, WaiterSnapshot, WorkspaceResourceManager, acquire_browser_for_current_session,
    acquire_file_write_for_current_session, acquire_file_write_guard, acquire_file_write_locked,
    acquire_many_file_write_guards, acquire_many_file_writes_locked,
    acquire_shell_for_current_session, acquire_workspace_exclusive_for_current_session,
    current_connection_id, current_session_context,
    global_workspace_resources,
    has_read_in_current_session, install_global as install_global_workspace_resources,
    is_stale_for_current_session,
    record_observed_for_current_session, record_read_for_current_session,
    record_write_for_current_session, scope_session_context,
    stale_file_error_message, subagent_session_context, subagent_session_context_at,
};
pub use run_state::{
    SessionRunGuard, SessionRunStateEvent, SessionRunStateRegistry, is_session_running_global,
};
pub use turn_feed::{
    SessionTurnFeed, TurnFeedGuard, deregister_turn_feed, get_turn_feed, register_turn_feed,
};
pub use workspace_run::{normalize_workspace_key, workspace_key_from_path};
pub use chat_view::{
    ChatEntry, ChatEntryKind, ChatViewSink, ChatViewSurface, SessionChatState,
    apply_session_event, apply_session_event_cli, apply_session_event_gui,
    apply_session_event_tui, replay_state_into_sink, spawn_hub_subscriber,
};
pub use event::{SessionEvent, SessionEventKind};
pub use persistence::{SNAPSHOT_EVERY, SessionEventLog};
pub use shell::{CliFormat, GuiEvent, TuiLine, TuiStyle, render_cli, render_gui, render_tui};
pub use state::{
    AgentId, EditBatchRef, RemoteDelta, SessionActor, SessionDelta, SessionId, SessionMetrics,
    SessionState, Turn,
};
pub use sync::SessionSyncHub;
pub use translators::session_to_agent_events;

use std::sync::Arc;

use tokio::sync::{Mutex, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::agent::agent::{Agent, TurnEvent};

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub model: String,
    pub temperature: f64,
    pub max_turns: Option<u32>,
    pub system_prompt_append: Option<String>,

    pub agent_id: Option<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            temperature: 0.7,
            max_turns: None,
            system_prompt_append: None,
            agent_id: None,
        }
    }
}

pub struct AgentSession {
    config: SessionConfig,
    event_tx: broadcast::Sender<SessionEvent>,
    cancel: CancellationToken,

    agent: Option<Arc<Mutex<Agent>>>,

    state: Option<Arc<SessionActor>>,
}

impl AgentSession {

    pub fn new(config: SessionConfig) -> (Self, broadcast::Receiver<SessionEvent>) {
        let (event_tx, event_rx) = broadcast::channel(256);
        let cancel = CancellationToken::new();

        let session = Self {
            config,
            event_tx,
            cancel,
            agent: None,
            state: None,
        };
        (session, event_rx)
    }

    pub fn with_agent(
        config: SessionConfig,
        agent: Arc<Mutex<Agent>>,
    ) -> (Self, broadcast::Receiver<SessionEvent>) {
        let (event_tx, event_rx) = broadcast::channel(256);
        let cancel = CancellationToken::new();

        let session = Self {
            config,
            event_tx,
            cancel,
            agent: Some(agent),
            state: None,
        };
        (session, event_rx)
    }

    pub fn with_agent_and_state(
        config: SessionConfig,
        agent: Arc<Mutex<Agent>>,
        state: Arc<SessionActor>,
    ) -> (Self, broadcast::Receiver<SessionEvent>) {
        let (session, rx) = Self::with_agent(config, agent);
        let session = session.attach_state(state);
        (session, rx)
    }

    #[must_use]
    pub fn attach_state(mut self, state: Arc<SessionActor>) -> Self {
        self.state = Some(state);
        self
    }

    pub fn state(&self) -> Option<Arc<SessionActor>> {
        self.state.clone()
    }

    pub fn has_agent(&self) -> bool {
        self.agent.is_some()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.event_tx.subscribe()
    }

    pub fn sink(&self) -> SessionEventSink {
        SessionEventSink::new(self.event_tx.clone())
    }

    pub async fn submit(&self, input: &str) -> Result<(), anyhow::Error> {
        let turn_start = std::time::Instant::now();
        let agent_id = self
            .config
            .agent_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        self.publish_event(SessionEvent::new(SessionEventKind::TurnStarted {
            input: input.to_string(),
        }));

        let Some(ref agent) = self.agent else {

            self.publish_event(SessionEvent::new(SessionEventKind::TurnFinished {
                output: format!("[session] received: {}", input),
                tokens_used: 0,
            }));
            return Ok(());
        };

        let (tx, mut rx) = mpsc::channel::<TurnEvent>(128);

        let event_tx = self.event_tx.clone();
        let state_for_bridge = self.state.clone();
        let agent_id_owned = agent_id.clone();
        let bridge_task =
            crate::runtime::spawn_supervised("agent_session.event_bridge", async move {
                let mut saw_first_token = false;
                let mut tool_id_pairer = FallbackToolIdPairer::default();
                while let Some(turn_event) = rx.recv().await {
                    if !saw_first_token && is_first_token_trigger(&turn_event) {
                        saw_first_token = true;
                        let elapsed = turn_start.elapsed();
                        let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
                        let first_tok = SessionEvent::new(SessionEventKind::FirstToken {
                            agent_id: agent_id_owned.clone(),
                            elapsed_ms,
                        });
                        if let Some(state) = &state_for_bridge {
                            state.apply(&first_tok);
                        }
                        let _ = event_tx.send(first_tok);
                        if let Some(observer) = crate::observability::global_observer() {
                            observer.record_metric(
                                &crate::observability::traits::ObserverMetric::FirstTokenLatency {
                                    agent_id: agent_id_owned.clone(),
                                    elapsed,
                                },
                            );
                        }
                    }
                    if let Some(sess_event) =
                        turn_event_to_session_event(turn_event, &mut tool_id_pairer)
                    {
                        if let Some(state) = &state_for_bridge {
                            state.apply(&sess_event);
                        }
                        let _ = event_tx.send(sess_event);
                    }
                }
            })
            .into_inner();

        let (turn_result, tokens_used) = {
            use futures_util::FutureExt as _;
            let mut guard = agent.lock().await;
            guard.reset_cancel();
            let result = match std::panic::AssertUnwindSafe(guard.turn_streamed(input, tx))
                .catch_unwind()
                .await
            {
                Ok(inner) => inner.map_err(|e| e.to_string()),
                Err(panic) => Err(format!(
                    "internal error recovered: {}",
                    crate::util::describe_panic(&*panic)
                )),
            };
            let tokens = guard
                .last_usage()
                .map(|usage| {
                    usage
                        .input_tokens
                        .unwrap_or(0)
                        .saturating_add(usage.output_tokens.unwrap_or(0))
                })
                .unwrap_or(0);
            (result, tokens)
        };

        let _ = bridge_task.await;

        let turn_error = turn_result.as_ref().err().cloned();
        let final_output = match turn_result {
            Ok(text) => text,
            Err(msg) => {
                self.publish_event(SessionEvent::new(SessionEventKind::Error {
                    message: msg.clone(),
                }));
                msg
            }
        };

        self.publish_event(SessionEvent::new(SessionEventKind::TurnFinished {
            output: final_output,
            tokens_used,
        }));

        match turn_error {
            Some(msg) => Err(anyhow::anyhow!(msg)),
            None => Ok(()),
        }
    }

    pub async fn submit_cancellable(&self, input: &str, cancel: CancellationToken) -> bool {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => false,
            _ = self.submit(input) => true,
        }
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    pub fn publish_event(&self, evt: SessionEvent) {
        if let Some(state) = &self.state {
            state.apply(&evt);
        }
        let _ = self.event_tx.send(evt);
    }

    pub fn approve(&self, approval_id: impl Into<String>, decision: impl Into<String>) {
        self.publish_event(SessionEvent::new(SessionEventKind::ApprovalResponded {
            id: approval_id.into(),
            decision: decision.into(),
            responder: self.config.agent_id.clone(),
            updated_input: None,
        }));
        crate::observability::session_write_mode_metrics::incr_approval_responded_via_session();
    }

    pub fn approve_with_input(
        &self,
        approval_id: impl Into<String>,
        decision: impl Into<String>,
        updated_input: Option<serde_json::Value>,
    ) {
        self.publish_event(SessionEvent::new(SessionEventKind::ApprovalResponded {
            id: approval_id.into(),
            decision: decision.into(),
            responder: self.config.agent_id.clone(),
            updated_input,
        }));
        crate::observability::session_write_mode_metrics::incr_approval_responded_via_session();
    }
}

fn is_first_token_trigger(event: &TurnEvent) -> bool {
    match event {
        TurnEvent::Chunk { delta } | TurnEvent::Thinking { delta } => !delta.is_empty(),
        _ => false,
    }
}

fn fallback_tool_call_id(name: &str) -> String {
    format!("{name}_{}", uuid::Uuid::new_v4())
}

#[derive(Default)]
pub struct FallbackToolIdPairer {
    by_name: std::collections::HashMap<String, std::collections::VecDeque<String>>,
}

impl FallbackToolIdPairer {
    fn on_call(&mut self, name: &str) -> String {
        let id = fallback_tool_call_id(name);
        self.push_call_id(name, id.clone());
        id
    }

    fn on_result(&mut self, name: &str) -> String {
        self.pop_result_id(name)
            .unwrap_or_else(|| fallback_tool_call_id(name))
    }

    pub fn push_call_id(&mut self, name: &str, id: String) {
        self.by_name
            .entry(name.to_string())
            .or_default()
            .push_back(id);
    }

    pub fn pop_result_id(&mut self, name: &str) -> Option<String> {
        self.by_name.get_mut(name).and_then(|q| q.pop_front())
    }

    pub fn remove_id(&mut self, name: &str, id: &str) {
        if let Some(q) = self.by_name.get_mut(name) {
            if let Some(pos) = q.iter().position(|existing| existing == id) {
                q.remove(pos);
            }
        }
    }

    pub fn peek_last(&self, name: &str) -> Option<String> {
        self.by_name.get(name).and_then(|q| q.back().cloned())
    }
}

pub fn turn_event_to_session_event(
    event: TurnEvent,
    pairer: &mut FallbackToolIdPairer,
) -> Option<SessionEvent> {
    let kind = match event {
        TurnEvent::Chunk { delta } => SessionEventKind::Delta { text: delta },
        TurnEvent::Thinking { delta } => SessionEventKind::Thinking { text: delta },
        TurnEvent::ToolCall {
            name,
            args,
            tool_call_id,
        } => SessionEventKind::ToolCall {
            tool_name: name.clone(),
            tool_call_id: tool_call_id
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| pairer.on_call(&name)),
            arguments: args,
        },
        TurnEvent::ToolResult {
            name,
            output,
            success,
            tool_call_id,
        } => {
            let is_error = !success
                || crate::agent::tool_handler::event_status::output_indicates_error(&output);
            SessionEventKind::ToolResult {
                tool_call_id: tool_call_id
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| pairer.on_result(&name)),
                output,
                is_error,
            }
        }
        TurnEvent::Error { message } => SessionEventKind::Error { message },

        TurnEvent::FileEdit {
            path,
            additions,
            deletions,
            ..
        } => SessionEventKind::FileEdit {
            path,
            additions,
            deletions,
        },
        TurnEvent::StatusUpdate { action, detail: _ } => {

            if action == "compressed" {
                SessionEventKind::ContextCompressed {
                    tokens_before: 0,
                    tokens_after: 0,
                }
            } else {
                return None;
            }
        }
        TurnEvent::PermissionRequest {
            request_id,
            tool_name,
            input,
            description,
        } => {
            if matches!(tool_name.as_str(), "ask_question" | "ask_user" | "AskQuestion") {
                return None;
            }
            let arguments = match description {
                Some(desc) => serde_json::json!({
                    "description": desc,
                    "input": input,
                }),
                None => input,
            };
            SessionEventKind::ApprovalRequested {
                id: request_id,
                tool_name,
                arguments,
                issued_at: chrono::Utc::now(),
            }
        }
        TurnEvent::StreamReset => SessionEventKind::StreamReset,
        TurnEvent::DraftCheckpoint
        | TurnEvent::ProgressTick { .. }
        | TurnEvent::CommandPreview { .. }
        | TurnEvent::Cancelling { .. }
        | TurnEvent::PiiSanitized { .. }
        | TurnEvent::PlanProgressCommitted { .. }
        | TurnEvent::ToolArgsDelta { .. } => {

            return None;
        }
        TurnEvent::ProviderRetry {
            attempt,
            max_attempts,
            wait_ms,
            class,
            provider,
            model,
            message,
        } => SessionEventKind::ProviderRetry {
            attempt,
            max_attempts,
            wait_ms,
            class,
            provider,
            model,
            message,
        },
        TurnEvent::ContextCompressed {
            tokens_before,
            tokens_after,
        } => SessionEventKind::ContextCompressed {
            tokens_before,
            tokens_after,
        },
        TurnEvent::SubagentChunk {
            task_id,
            agent_id,
            kind: subkind,
            delta,
        } => {

            let label = format!("[{agent_id}::{task_id}]");
            SessionEventKind::Delta {
                text: match subkind {
                    crate::agent::SubagentChunkKind::Chunk => {
                        format!("{label} {delta}")
                    }
                    crate::agent::SubagentChunkKind::Thinking => {
                        format!("{label} [thinking] {delta}")
                    }
                    crate::agent::SubagentChunkKind::ToolCall => {
                        format!("{label} -> tool {delta}")
                    }
                    crate::agent::SubagentChunkKind::ToolResult => {
                        format!("{label} <- {delta}")
                    }
                    crate::agent::SubagentChunkKind::Status => {
                        format!("{label} {delta}")
                    }
                },
            }
        }
        TurnEvent::WorkerSpawned {
            parent_tool_use_id,
            worker_id,
            title,
            model,
        } => SessionEventKind::WorkerSpawned {
            parent_tool_use_id,
            worker_id,
            title,
            model,
        },
        TurnEvent::WorkerStatus {
            worker_id,
            status,
            detail,
        } => SessionEventKind::WorkerStatus {
            worker_id,
            status,
            detail,
        },
        TurnEvent::WorkerProgress {
            worker_id,
            action,
            detail,
        } => SessionEventKind::WorkerProgress {
            worker_id,
            action,
            detail,
        },
        TurnEvent::WorkerCompleted {
            worker_id,
            success,
            summary,
        } => SessionEventKind::WorkerCompleted {
            worker_id,
            success,
            summary,
        },
        TurnEvent::WorkerStopped { worker_id, reason } => {
            SessionEventKind::WorkerStopped { worker_id, reason }
        }
        TurnEvent::ParentResumed { reason } => SessionEventKind::ParentResumed { reason },
    };
    Some(SessionEvent::new(kind))
}
