// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;

use axum::{
    extract::{
        Path, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

use crate::agent::{SubagentChunkKind, TurnEvent};
use crate::session::event::{SessionEvent, SessionEventKind};
use crate::workers::persistence::replay_worker_events;
use crate::workers::supervisor::global_supervisor;

pub async fn handle_ws_worker(
    Path(worker_id): Path<String>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(reject) = crate::gateway::cors::reject_ws_disallowed_origin(&headers, "/ws/worker")
    {
        return reject;
    }
    ws.on_upgrade(move |socket| run_worker_socket(socket, worker_id))
        .into_response()
}

async fn run_worker_socket(socket: WebSocket, worker_id: String) {
    let (mut sink, mut stream) = socket.split();

    let _ = sink
        .send(Message::Text(
            json!({
                "type": "connected",
                "sessionId": worker_id,
            })
            .to_string()
            .into(),
        ))
        .await;

    let supervisor = global_supervisor();
    let workspace_root = supervisor
        .as_ref()
        .and_then(|s| s.get(&worker_id))
        .map(|h| h.workspace_root.clone())
        .or_else(|| {
            crate::workers::persistence::find_worker_root(
                &crate::workers::supervisor::candidate_worker_roots(),
                &worker_id,
            )
        })
        .or_else(|| {
            supervisor
                .as_ref()
                .map(|s| s.workspace_root().to_path_buf())
        })
        .or_else(|| crate::bootstrap::try_get_state().map(|st| st.read(|s| s.cwd.clone())))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let handle = match supervisor.as_ref().and_then(|s| s.get(&worker_id)) {
        Some(h) => h,
        None => {
            let mut terminal_sent = false;
            if let Ok(events) = replay_worker_events(&workspace_root, &worker_id) {
                for record in events {
                    terminal_sent |= session_event_is_terminal(&record.event);
                    for frame in session_event_to_wire(&worker_id, &record.event) {
                        if sink
                            .send(Message::Text(frame.to_string().into()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            if terminal_sent {
                let _ = sink.close().await;
                return;
            }
            let _ = sink
                .send(Message::Text(
                    json!({
                        "type": "system_notification",
                        "subtype": "worker_replay_only",
                        "data": {
                            "workerId": worker_id,
                            "message": "Worker is no longer active; only historical events were sent."
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await;
            return;
        }
    };

    let mut rx = handle.subscribe();
    let handle_for_cancel = handle.clone();

    let mut wire_tracker = WorkerWireTracker::default();
    let mut highwater = 0_u64;
    let mut terminal_replayed = false;
    if let Ok(events) = replay_worker_events(&workspace_root, &worker_id) {
        for record in events {
            highwater = highwater.max(record.seq);
            terminal_replayed |= session_event_is_terminal(&record.event);
            wire_tracker.observe_session_event(&record.event);
            for frame in session_event_to_wire(&worker_id, &record.event) {
                if sink
                    .send(Message::Text(frame.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
    if terminal_replayed {
        let _ = sink.close().await;
        return;
    }

    loop {
        tokio::select! {
            recv = rx.recv() => {
                match recv {
                    Ok(live) => {
                        if live.seq <= highwater {
                            continue;
                        }
                        highwater = live.seq;
                        let terminal = turn_event_is_terminal(&live.event);
                        let frames = wire_tracker.turn_event_to_wire(&worker_id, &live.event);
                        for frame in frames {
                            if sink
                                .send(Message::Text(frame.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        if terminal {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if let Ok(events) = replay_worker_events(&workspace_root, &worker_id) {
                            let mut terminal = false;
                            for record in events {
                                if record.seq <= highwater {
                                    continue;
                                }
                                highwater = record.seq;
                                terminal |= session_event_is_terminal(&record.event);
                                wire_tracker.observe_session_event(&record.event);
                                for frame in session_event_to_wire(&worker_id, &record.event) {
                                    if sink
                                        .send(Message::Text(frame.to_string().into()))
                                        .await
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                            if terminal {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            incoming = stream.next() => {
                let Some(frame) = incoming else { break; };
                let Ok(Message::Text(text)) = frame else { continue; };
                let Ok(parsed) = serde_json::from_str::<Value>(text.as_str()) else { continue; };
                let msg_type = parsed
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match msg_type {
                    "ping" => {
                        let _ = sink
                            .send(Message::Text(r#"{"type":"pong"}"#.to_string().into()))
                            .await;
                    }
                    "stop_generation" | "stop" | "cancel" => {
                        handle_for_cancel.cancel();
                        let _ = sink
                            .send(Message::Text(
                                json!({
                                    "type": "system_notification",
                                    "subtype": "worker_cancel_requested",
                                    "data": { "workerId": worker_id }
                                })
                                .to_string()
                                .into(),
                            ))
                            .await;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = sink.close().await;
}

#[derive(Default)]
struct WorkerWireTracker {
    current_tool_use_id: Option<String>,
    tool_use_id_for_name: HashMap<String, String>,
}

impl WorkerWireTracker {
    fn next_tool_use_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn resolve_tool_call_id(&mut self, name: &str, tool_call_id: &Option<String>) -> String {
        let id = tool_call_id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(Self::next_tool_use_id);
        self.current_tool_use_id = Some(id.clone());
        self.tool_use_id_for_name
            .insert(name.to_string(), id.clone());
        id
    }

    fn resolve_tool_result_id(&mut self, name: &str, tool_call_id: &Option<String>) -> String {
        let id = tool_call_id
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| self.tool_use_id_for_name.remove(name))
            .or_else(|| self.current_tool_use_id.clone())
            .unwrap_or_else(Self::next_tool_use_id);
        self.current_tool_use_id = None;
        id
    }

    fn observe_session_event(&mut self, event: &SessionEvent) {
        match &event.kind {
            SessionEventKind::ToolCall {
                tool_name,
                tool_call_id,
                ..
            } => {
                self.current_tool_use_id = Some(tool_call_id.clone());
                self.tool_use_id_for_name
                    .insert(tool_name.clone(), tool_call_id.clone());
            }
            SessionEventKind::ToolResult { tool_call_id, .. } => {
                self.tool_use_id_for_name
                    .retain(|_, value| value != tool_call_id);
                if self.current_tool_use_id.as_ref() == Some(tool_call_id) {
                    self.current_tool_use_id = None;
                }
            }
            _ => {}
        }
    }

    fn turn_event_to_wire(&mut self, session_id: &str, event: &TurnEvent) -> Vec<Value> {
        match event {
            TurnEvent::Chunk { delta } => vec![json!({
                "type": "content_delta",
                "delta": { "type": "text_delta", "text": delta },
            })],
            TurnEvent::StreamReset => vec![json!({
                "type": "content_reset",
            })],
            TurnEvent::Thinking { delta } => vec![json!({
                "type": "thinking",
                "text": delta,
            })],
            TurnEvent::ToolCall {
                name,
                args,
                tool_call_id,
            } => {
                let id = self.resolve_tool_call_id(name, tool_call_id);
                let safe_args =
                    crate::services::governance::credential_vault::redact_args_optional(args);
                vec![
                    json!({
                        "type": "content_start",
                        "blockType": "tool_use",
                        "toolName": name,
                        "toolUseId": id,
                    }),
                    json!({
                        "type": "tool_use_complete",
                        "toolName": name,
                        "toolUseId": id,
                        "input": safe_args,
                        "sessionId": session_id,
                    }),
                ]
            }
            TurnEvent::ToolResult {
                name,
                output,
                success,
                tool_call_id,
            } => {
                let id = self.resolve_tool_result_id(name, tool_call_id);
                let is_error = !success
                    || crate::agent::tool_handler::event_status::output_indicates_error(output);
                let safe_output =
                    crate::services::governance::credential_vault::redact_for_audit_optional(
                        output,
                    );
                vec![json!({
                    "type": "tool_result",
                    "toolUseId": id,
                    "content": safe_output,
                    "isError": is_error,
                })]
            }
            TurnEvent::StatusUpdate { action, detail } => {
                let mut frames = vec![json!({
                    "type": "status",
                    "state": "tool_executing",
                    "verb": action,
                    "tokens": null,
                })];
                if !detail.is_empty() {
                    frames.push(json!({
                        "type": "system_notification",
                        "subtype": "status_detail",
                        "message": detail,
                    }));
                }
                frames
            }
            TurnEvent::Error { message } => vec![json!({
                "type": "error",
                "message": message,
                "errorCode": "WORKER_ERROR",
            })],
            TurnEvent::FileEdit {
                path,
                additions,
                deletions,
                diff,
                edit_batch_id,
            } => vec![json!({
                "type": "system_notification",
                "subtype": "file_edit",
                "data": {
                    "path": path,
                    "additions": additions,
                    "deletions": deletions,
                    "diff": diff,
                    "editBatchId": edit_batch_id,
                }
            })],
            TurnEvent::SubagentChunk {
                task_id,
                agent_id,
                kind,
                delta,
            } => vec![json!({
                "type": "system_notification",
                "subtype": "subagent_chunk",
                "data": {
                    "taskId": task_id,
                    "agentId": agent_id,
                    "kind": match kind {
                        SubagentChunkKind::Chunk => "chunk",
                        SubagentChunkKind::Thinking => "thinking",
                        SubagentChunkKind::ToolCall => "tool_call",
                        SubagentChunkKind::ToolResult => "tool_result",
                        SubagentChunkKind::Status => "status",
                    },
                    "delta": delta,
                }
            })],
            TurnEvent::ProgressTick {
                iteration,
                max_iterations: _,
                tokens_used,
            } => vec![json!({
                "type": "status",
                "state": "thinking",
                "verb": format!("iter {iteration}"),
                "tokens": tokens_used,
            })],
            TurnEvent::CommandPreview {
                tool_name,
                args,
                estimated_duration_ms: _,
            } => vec![json!({
                "type": "system_notification",
                "subtype": "command_preview",
                "data": {
                    "toolName": tool_name,
                    "input": args,
                }
            })],
            TurnEvent::Cancelling { reason } => vec![
                json!({
                    "type": "status",
                    "state": "idle",
                    "verb": "cancelling",
                }),
                json!({
                    "type": "system_notification",
                    "subtype": "cancelling",
                    "message": reason,
                }),
            ],
            TurnEvent::PermissionRequest {
                request_id,
                tool_name,
                input,
                description,
            } => {
                let tool_use_id = self
                    .tool_use_id_for_name
                    .get(tool_name)
                    .cloned()
                    .or_else(|| self.current_tool_use_id.clone());
                let mut frame = json!({
                    "type": "permission_request",
                    "requestId": request_id,
                    "toolName": tool_name,
                    "input": input,
                });
                if let Some(id) = tool_use_id {
                    frame["toolUseId"] = Value::String(id);
                }
                if let Some(desc) = description {
                    frame["description"] = Value::String(desc.clone());
                }
                vec![frame]
            }
            TurnEvent::ProviderRetry {
                attempt,
                max_attempts,
                wait_ms,
                class,
                provider,
                model,
                message,
            } => vec![json!({
                "type": "provider_retry",
                "attempt": attempt,
                "maxAttempts": max_attempts,
                "waitMs": wait_ms,
                "class": class,
                "provider": provider,
                "model": model,
                "message": message,
            })],
            TurnEvent::WorkerSpawned {
                parent_tool_use_id,
                worker_id,
                title,
                model,
            } => vec![json!({
                "type": "worker_spawned",
                "parentToolUseId": parent_tool_use_id,
                "workerId": worker_id,
                "title": title,
                "model": model,
            })],
            TurnEvent::WorkerStatus {
                worker_id,
                status,
                detail,
            } => vec![json!({
                "type": "worker_status",
                "workerId": worker_id,
                "status": status,
                "detail": detail,
            })],
            TurnEvent::WorkerProgress {
                worker_id,
                action,
                detail,
            } => vec![json!({
                "type": "worker_progress",
                "workerId": worker_id,
                "action": action,
                "detail": detail,
            })],
            TurnEvent::WorkerCompleted {
                worker_id,
                success,
                summary,
            } => vec![json!({
                "type": "worker_completed",
                "workerId": worker_id,
                "success": success,
                "summary": summary,
            })],
            TurnEvent::WorkerStopped { worker_id, reason } => vec![json!({
                "type": "worker_stopped",
                "workerId": worker_id,
                "reason": reason,
            })],
            TurnEvent::ParentResumed { reason } => vec![json!({
                "type": "parent_resumed",
                "reason": reason,
            })],
            TurnEvent::ContextCompressed {
                tokens_before,
                tokens_after,
            } => vec![json!({
                "type": "context_compressed",
                "tokensBefore": tokens_before,
                "tokensAfter": tokens_after,
            })],
            TurnEvent::PlanProgressCommitted {
                plan_path,
                title,
                todos_json,
            } => vec![json!({
                "type": "plan_progress_committed",
                "planPath": plan_path,
                "title": title,
                "todos": todos_json,
            })],
            TurnEvent::PiiSanitized { .. } => vec![json!({
                "type": "pii_sanitized",
            })],
        }
    }
}

fn session_event_to_wire(session_id: &str, event: &SessionEvent) -> Vec<Value> {
    match &event.kind {
        SessionEventKind::Delta { text } => vec![json!({
            "type": "content_delta",
            "delta": { "type": "text_delta", "text": text },
        })],
        SessionEventKind::ToolCall {
            tool_name,
            tool_call_id,
            arguments,
        } => vec![
            json!({
                "type": "content_start",
                "blockType": "tool_use",
                "toolName": tool_name,
                "toolUseId": tool_call_id,
            }),
            json!({
                "type": "tool_use_complete",
                "toolName": tool_name,
                "toolUseId": tool_call_id,
                "input": arguments,
                "sessionId": session_id,
            }),
        ],
        SessionEventKind::ToolResult {
            tool_call_id,
            output,
            is_error,
        } => vec![json!({
            "type": "tool_result",
            "toolUseId": tool_call_id,
            "content": output,
            "isError": is_error,
        })],
        SessionEventKind::TurnFinished { output, .. } => vec![json!({
            "type": "turn_complete",
            "output": output,
        })],
        SessionEventKind::Error { message } => vec![json!({
            "type": "error",
            "message": message,
            "errorCode": "WORKER_ERROR",
        })],
        SessionEventKind::WorkerSpawned {
            parent_tool_use_id,
            worker_id,
            title,
            model,
        } => vec![json!({
            "type": "worker_spawned",
            "parentToolUseId": parent_tool_use_id,
            "workerId": worker_id,
            "title": title,
            "model": model,
        })],
        SessionEventKind::WorkerStatus {
            worker_id,
            status,
            detail,
        } => vec![json!({
            "type": "worker_status",
            "workerId": worker_id,
            "status": status,
            "detail": detail,
        })],
        SessionEventKind::WorkerProgress {
            worker_id,
            action,
            detail,
        } => vec![json!({
            "type": "worker_progress",
            "workerId": worker_id,
            "action": action,
            "detail": detail,
        })],
        SessionEventKind::WorkerCompleted {
            worker_id,
            success,
            summary,
        } => vec![json!({
            "type": "worker_completed",
            "workerId": worker_id,
            "success": success,
            "summary": summary,
        })],
        SessionEventKind::WorkerStopped { worker_id, reason } => vec![json!({
            "type": "worker_stopped",
            "workerId": worker_id,
            "reason": reason,
        })],
        SessionEventKind::TurnStarted { .. }
        | SessionEventKind::FirstToken { .. }
        | SessionEventKind::ContextCompressed { .. }
        | SessionEventKind::ModeChanged { .. }
        | SessionEventKind::WritePlanCreated { .. }
        | SessionEventKind::WriteStepStarted { .. }
        | SessionEventKind::WriteStepFinished { .. }
        | SessionEventKind::WriteVerify { .. }
        | SessionEventKind::DiffSessionApplied { .. }
        | SessionEventKind::DiffSessionRolledBack { .. }
        | SessionEventKind::ApprovalRequested { .. }
        | SessionEventKind::ApprovalResponded { .. }
        | SessionEventKind::CheckpointCreated { .. }
        | SessionEventKind::OpenFileMarked { .. }
        | SessionEventKind::ProviderRetry { .. }
        | SessionEventKind::ParentResumed { .. } => Vec::new(),
    }
}

fn session_event_is_terminal(event: &SessionEvent) -> bool {
    matches!(
        &event.kind,
        SessionEventKind::WorkerCompleted { .. } | SessionEventKind::WorkerStopped { .. }
    )
}

fn turn_event_is_terminal(event: &TurnEvent) -> bool {
    matches!(
        event,
        TurnEvent::WorkerCompleted { .. } | TurnEvent::WorkerStopped { .. }
    )
}
