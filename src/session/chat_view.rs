// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::broadcast;

use super::{SessionEvent, SessionEventKind};
use crate::observability::session_write_mode_metrics;

#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub kind: ChatEntryKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatEntryKind {
    User,
    Assistant,
    ToolCall,
    ToolResult,
    ToolError,
    Error,
    System,
}

const MAX_ENTRIES: usize = 500;

pub struct SessionChatState {
    entries: Arc<Mutex<VecDeque<ChatEntry>>>,

    last_assistant_pending: Arc<Mutex<bool>>,
}

impl SessionChatState {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_ENTRIES + 16))),
            last_assistant_pending: Arc::new(Mutex::new(false)),
        }
    }

    pub fn spawn_drain(
        &self,
        mut rx: broadcast::Receiver<SessionEvent>,
    ) -> crate::runtime::TaskHandle {
        let entries = self.entries.clone();
        let pending = self.last_assistant_pending.clone();
        crate::runtime::spawn_supervised("agent_session.chat_view.drain", async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let mut guard = entries.lock();
                        let mut pend = pending.lock();
                        apply_event(&mut guard, &mut pend, event);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    pub fn snapshot(&self) -> Vec<ChatEntry> {
        self.entries.lock().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
        *self.last_assistant_pending.lock() = false;
    }

    pub fn record_user(&self, input: &str) {
        let mut guard = self.entries.lock();
        push_with_cap(
            &mut guard,
            ChatEntry {
                kind: ChatEntryKind::User,
                text: input.to_string(),
            },
        );
        *self.last_assistant_pending.lock() = false;
    }

    pub fn entry_count(&self) -> usize {
        self.entries.lock().len()
    }
}

impl Default for SessionChatState {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_event(entries: &mut VecDeque<ChatEntry>, pending: &mut bool, event: SessionEvent) {
    match event.kind {
        SessionEventKind::TurnStarted { .. } => {

            *pending = false;
        }
        SessionEventKind::Thinking { .. } => {}
        SessionEventKind::StreamReset => {
            if *pending {
                if let Some(last) = entries.back() {
                    if last.kind == ChatEntryKind::Assistant {
                        entries.pop_back();
                    }
                }
                *pending = false;
            }
        }
        SessionEventKind::FileEdit {
            path,
            additions,
            deletions,
        } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(edited {path} +{additions}/-{deletions})"),
                },
            );
        }
        SessionEventKind::Delta { text } => {
            if *pending {
                if let Some(last) = entries.back_mut() {
                    if last.kind == ChatEntryKind::Assistant {
                        last.text.push_str(&text);
                        return;
                    }
                }
            }
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::Assistant,
                    text,
                },
            );
            *pending = true;
        }
        SessionEventKind::ToolCall {
            tool_name,
            arguments,
            ..
        } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::ToolCall,
                    text: format!(
                        "{tool_name}({})",
                        truncate_preview(&arguments.to_string(), 120)
                    ),
                },
            );
            *pending = false;
        }
        SessionEventKind::ToolResult {
            output, is_error, ..
        } => {
            let kind = if is_error {
                ChatEntryKind::ToolError
            } else {
                ChatEntryKind::ToolResult
            };
            push_with_cap(
                entries,
                ChatEntry {
                    kind,
                    text: truncate_preview(&output, 200),
                },
            );
            *pending = false;
        }
        SessionEventKind::TurnFinished { output, .. } => {

            if !*pending && !output.is_empty() {
                push_with_cap(
                    entries,
                    ChatEntry {
                        kind: ChatEntryKind::Assistant,
                        text: output,
                    },
                );
            }
            *pending = false;
        }
        SessionEventKind::Error { message } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::Error,
                    text: message,
                },
            );
            *pending = false;
        }
        SessionEventKind::ContextCompressed {
            tokens_before,
            tokens_after,
        } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(context compressed: {tokens_before} ??{tokens_after} tokens)"),
                },
            );
        }
        SessionEventKind::ModeChanged { mode } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(mode changed: {mode})"),
                },
            );
        }
        SessionEventKind::FirstToken {
            agent_id,
            elapsed_ms,
        } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(first token for {agent_id}: {elapsed_ms} ms)"),
                },
            );
        }

        SessionEventKind::WritePlanCreated { .. }
        | SessionEventKind::WriteStepStarted { .. }
        | SessionEventKind::WriteStepFinished { .. }
        | SessionEventKind::WriteVerify { .. }
        | SessionEventKind::DiffSessionApplied { .. }
        | SessionEventKind::DiffSessionRolledBack { .. } => {}
        SessionEventKind::ProviderRetry { .. } => {}
        SessionEventKind::ApprovalRequested {
            id, tool_name, ..
        } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(approval requested: {tool_name} id={id})"),
                },
            );
        }
        SessionEventKind::ApprovalResponded {
            id,
            decision,
            responder,
            updated_input: _,
        } => {
            let who = responder.as_deref().unwrap_or("unknown");
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(approval {id} ??{decision} by {who})"),
                },
            );
        }
        SessionEventKind::CheckpointCreated {
            cp_id,
            edit_batch_id,
        } => {
            let suffix = edit_batch_id
                .as_ref()
                .map(|b| format!(" ??batch {b}"))
                .unwrap_or_default();
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(checkpoint {cp_id}{suffix})"),
                },
            );
        }
        SessionEventKind::OpenFileMarked {
            path,
            cursor,
            source,
        } => {
            let cursor_hint = cursor
                .map(|(l, c)| format!(" @ {l}:{c}"))
                .unwrap_or_default();
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(opened {path}{cursor_hint} via {source})"),
                },
            );
        }
        SessionEventKind::WorkerSpawned {
            worker_id, title, ..
        } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(worker spawned: {worker_id} '{title}')"),
                },
            );
        }
        SessionEventKind::WorkerStatus { .. }
        | SessionEventKind::WorkerProgress { .. } => {}
        SessionEventKind::WorkerCompleted {
            worker_id, success, summary,
        } => {
            let label = if success { "completed" } else { "failed" };
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(worker {worker_id} {label}: {summary})"),
                },
            );
        }
        SessionEventKind::WorkerStopped { worker_id, reason } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(worker {worker_id} stopped: {reason})"),
                },
            );
        }
        SessionEventKind::ParentResumed { reason } => {
            push_with_cap(
                entries,
                ChatEntry {
                    kind: ChatEntryKind::System,
                    text: format!("(parent resumed: {reason})"),
                },
            );
        }
    }
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.replace('\n', " ")
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('\u{2026}');
        out.replace('\n', " ")
    }
}

fn push_with_cap(entries: &mut VecDeque<ChatEntry>, entry: ChatEntry) {
    if entries.len() >= MAX_ENTRIES {
        entries.pop_front();
    }
    entries.push_back(entry);
}

pub trait ChatViewSink {

    fn push_user(&mut self, text: &str);

    fn append_assistant_delta(&mut self, text: &str);

    fn close_assistant_turn(&mut self, output: &str);

    fn push_tool_call(&mut self, tool_name: &str, arguments: &serde_json::Value);
    fn push_tool_result(&mut self, output: &str, is_error: bool);
    fn push_error(&mut self, message: &str);
    fn push_system(&mut self, message: &str);

    fn reset(&mut self) {}
}

#[derive(Debug, Default)]
pub struct NullChatViewSink;

impl ChatViewSink for NullChatViewSink {
    fn push_user(&mut self, _text: &str) {}
    fn append_assistant_delta(&mut self, _text: &str) {}
    fn close_assistant_turn(&mut self, _output: &str) {}
    fn push_tool_call(&mut self, _tool_name: &str, _arguments: &serde_json::Value) {}
    fn push_tool_result(&mut self, _output: &str, _is_error: bool) {}
    fn push_error(&mut self, _message: &str) {}
    fn push_system(&mut self, _message: &str) {}
}

pub fn apply_session_event<S: ChatViewSink + ?Sized>(sink: &mut S, evt: &SessionEvent) {
    match &evt.kind {
        SessionEventKind::TurnStarted { input } => {
            sink.push_user(input);
        }
        SessionEventKind::Delta { text } => {
            sink.append_assistant_delta(text);
        }
        SessionEventKind::Thinking { .. } => {}
        SessionEventKind::StreamReset => {
            sink.push_system("stream reset: provider retried; partial output discarded");
        }
        SessionEventKind::FileEdit {
            path,
            additions,
            deletions,
        } => {
            sink.push_system(&format!("edited {path} (+{additions}/-{deletions})"));
        }
        SessionEventKind::ToolCall {
            tool_name,
            arguments,
            ..
        } => {
            sink.push_tool_call(tool_name, arguments);
        }
        SessionEventKind::ToolResult {
            output, is_error, ..
        } => {
            sink.push_tool_result(output, *is_error);
        }
        SessionEventKind::TurnFinished { output, .. } => {
            sink.close_assistant_turn(output);
        }
        SessionEventKind::Error { message } => {
            sink.push_error(message);
        }
        SessionEventKind::ContextCompressed {
            tokens_before,
            tokens_after,
        } => {
            sink.push_system(&format!(
                "context compressed: {tokens_before} ??{tokens_after} tokens"
            ));
        }
        SessionEventKind::ModeChanged { mode } => {
            sink.push_system(&format!("mode changed: {mode}"));
        }
        SessionEventKind::FirstToken {
            agent_id,
            elapsed_ms,
        } => {
            sink.push_system(&format!(
                "first token for {agent_id}: {elapsed_ms} ms"
            ));
        }
        SessionEventKind::ApprovalRequested {
            id, tool_name, ..
        } => {
            sink.push_system(&format!(
                "approval requested: {tool_name} (id={id})"
            ));
        }
        SessionEventKind::ApprovalResponded {
            id,
            decision,
            responder,
            updated_input: _,
        } => {
            let who = responder.as_deref().unwrap_or("unknown");
            sink.push_system(&format!(
                "approval {id} ??{decision} (by {who})"
            ));
        }
        SessionEventKind::CheckpointCreated {
            cp_id,
            edit_batch_id,
        } => {
            if let Some(batch) = edit_batch_id {
                sink.push_system(&format!(
                    "checkpoint {cp_id} ??edit batch {batch}"
                ));
            } else {
                sink.push_system(&format!("checkpoint {cp_id} created"));
            }
        }
        SessionEventKind::OpenFileMarked {
            path,
            cursor,
            source,
        } => {
            let hint = cursor
                .map(|(l, c)| format!(" @ {l}:{c}"))
                .unwrap_or_default();
            sink.push_system(&format!("opened {path}{hint} via {source}"));
        }
        SessionEventKind::WritePlanCreated { .. }
        | SessionEventKind::WriteStepStarted { .. }
        | SessionEventKind::WriteStepFinished { .. }
        | SessionEventKind::WriteVerify { .. }
        | SessionEventKind::DiffSessionApplied { .. }
        | SessionEventKind::DiffSessionRolledBack { .. } => {

        }
        SessionEventKind::ProviderRetry { .. } => {

        }
        SessionEventKind::WorkerSpawned {
            worker_id, title, ..
        } => {
            sink.push_system(&format!("worker spawned: {worker_id} '{title}'"));
        }
        SessionEventKind::WorkerStatus { .. }
        | SessionEventKind::WorkerProgress { .. } => {}
        SessionEventKind::WorkerCompleted {
            worker_id, success, summary,
        } => {
            let label = if *success { "completed" } else { "failed" };
            sink.push_system(&format!("worker {worker_id} {label}: {summary}"));
        }
        SessionEventKind::WorkerStopped { worker_id, reason } => {
            sink.push_system(&format!("worker {worker_id} stopped: {reason}"));
        }
        SessionEventKind::ParentResumed { reason } => {
            sink.push_system(&format!("parent resumed: {reason}"));
        }
    }
}

pub fn apply_session_event_cli<S: ChatViewSink>(sink: &mut S, evt: &SessionEvent) {
    apply_session_event(sink, evt);
    session_write_mode_metrics::incr_chat_view_reduce_cli();
}

pub fn apply_session_event_tui<S: ChatViewSink>(sink: &mut S, evt: &SessionEvent) {
    apply_session_event(sink, evt);
    session_write_mode_metrics::incr_chat_view_reduce_tui();
}

pub fn apply_session_event_gui<S: ChatViewSink>(sink: &mut S, evt: &SessionEvent) {
    apply_session_event(sink, evt);
    session_write_mode_metrics::incr_chat_view_reduce_gui();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatViewSurface {
    Cli,
    Tui,
    Gui,
}

pub fn spawn_hub_subscriber<S>(
    session_id: impl Into<String>,
    sink: Arc<parking_lot::Mutex<S>>,
    surface: ChatViewSurface,
) -> crate::runtime::TaskHandle
where
    S: ChatViewSink + Send + 'static,
{
    let session_id = session_id.into();
    let hub = super::SessionSyncHub::global();
    let mut rx = hub.subscribe(&session_id);
    crate::runtime::spawn_supervised("session.chat_view.hub_subscriber", async move {
        loop {
            match rx.recv().await {
                Ok(delta) => {
                    let mut guard = sink.lock();
                    match surface {
                        ChatViewSurface::Cli => apply_session_event_cli(&mut *guard, &delta.event),
                        ChatViewSurface::Tui => apply_session_event_tui(&mut *guard, &delta.event),
                        ChatViewSurface::Gui => apply_session_event_gui(&mut *guard, &delta.event),
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    let mut guard = sink.lock();
                    guard.push_system(&format!(
                        "view lagged behind by {skipped} update(s); some recent history may be delayed until the next live update"
                    ));
                    drop(guard);
                    tracing::warn!(
                        target: "session.sync",
                        session_id = %session_id,
                        skipped,
                        "hub subscriber lagged behind broadcast; emitted a lag marker instead of full-snapshot replay because chat view sinks cannot be cleared without duplicating already-rendered history"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

pub fn replay_state_into_sink<S>(state: &super::SessionState, sink: &mut S) -> u64
where
    S: ChatViewSink + ?Sized,
{
    for turn in &state.turns {
        sink.push_user(&turn.input);
        if let Some(out) = turn.output.as_ref() {
            if !out.is_empty() {
                sink.close_assistant_turn(out);
            }
        }
        for call in &turn.tool_calls {
            sink.push_tool_call(&call.tool_name, &call.arguments);
            if let Some(result) = call.result.as_ref() {
                sink.push_tool_result(result, call.is_error);
            }
        }
    }
    for appr in &state.pending_approvals {
        if appr.decision.is_none() {
            sink.push_system(&format!(
                "approval pending: {} (id={})",
                appr.tool_name, appr.id
            ));
        }
    }
    state.version
}
