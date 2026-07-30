// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::agent::bridge_types::AgentEvent;
use crate::session::{SessionEvent, SessionEventKind};

pub fn session_to_agent_events(event: &SessionEvent) -> Vec<AgentEvent> {
    match &event.kind {
        SessionEventKind::TurnStarted { .. } => vec![AgentEvent::Thinking],
        SessionEventKind::Delta { text } => vec![AgentEvent::StreamChunk(text.clone())],
        SessionEventKind::ToolCall {
            tool_name,
            tool_call_id,
            arguments,
        } => {
            let input = if arguments.is_null() {
                None
            } else {
                Some(arguments.to_string())
            };
            vec![AgentEvent::ToolUse {
                name: tool_name.clone(),
                id: tool_call_id.clone(),
                input,
            }]
        }
        SessionEventKind::ToolResult {
            tool_call_id,
            output,
            is_error,
        } => vec![AgentEvent::ToolResult {
            id: tool_call_id.clone(),
            output: output.clone(),
            success: !*is_error,
        }],
        SessionEventKind::TurnFinished { output, .. } => {
            if output.is_empty() {
                vec![AgentEvent::Done]
            } else {
                vec![
                    AgentEvent::AssistantMessage(output.clone()),
                    AgentEvent::Done,
                ]
            }
        }
        SessionEventKind::Error { message } => vec![AgentEvent::Error(message.clone())],
        SessionEventKind::ContextCompressed {
            tokens_before,
            tokens_after,
        } => vec![AgentEvent::StatusUpdate {
            action: "compressed".into(),
            detail: format!("context trimmed from {tokens_before} → {tokens_after} tokens"),
        }],
        SessionEventKind::ModeChanged { mode } => {
            vec![AgentEvent::ModeChanged(mode.clone())]
        }
        SessionEventKind::FirstToken {
            agent_id,
            elapsed_ms,
        } => vec![AgentEvent::StatusUpdate {
            action: "first_token".into(),
            detail: format!("{agent_id} first token after {elapsed_ms} ms"),
        }],
        SessionEventKind::WritePlanCreated {
            goal,
            summary,
            steps,
        } => vec![AgentEvent::StatusUpdate {
            action: "write_plan_created".into(),
            detail: format!("plan({steps} steps, {summary}): {goal}"),
        }],
        SessionEventKind::WriteStepStarted { index, label } => vec![AgentEvent::StatusUpdate {
            action: "write_step_started".into(),
            detail: format!("step {index} {label}"),
        }],
        SessionEventKind::WriteStepFinished {
            index,
            label,
            ok,
            summary,
        } => vec![AgentEvent::StatusUpdate {
            action: if *ok {
                "write_step_ok".into()
            } else {
                "write_step_fail".into()
            },
            detail: format!("step {index} {label}: {summary}"),
        }],
        SessionEventKind::WriteVerify { status } => vec![AgentEvent::StatusUpdate {
            action: "write_verify".into(),
            detail: status.clone(),
        }],
        SessionEventKind::DiffSessionApplied {
            files,
            hunks_exact,
            hunks_fuzzy,
        } => vec![AgentEvent::StatusUpdate {
            action: "diff_session_applied".into(),
            detail: format!("{files} files, {hunks_exact} exact hunks, {hunks_fuzzy} fuzzy"),
        }],
        SessionEventKind::DiffSessionRolledBack { files } => vec![AgentEvent::StatusUpdate {
            action: "diff_session_rolled_back".into(),
            detail: format!("{files} files"),
        }],
        SessionEventKind::ApprovalRequested {
            id,
            tool_name,
            arguments,
            ..
        } => vec![AgentEvent::ApprovalRequest {
            tool_name: tool_name.clone(),
            tool_id: id.clone(),
            args_summary: {
                let s = arguments.to_string();
                if s.chars().count() > 160 {
                    let mut t: String = s.chars().take(160).collect();
                    t.push('…');
                    t
                } else {
                    s
                }
            },
            arguments: arguments.clone(),
        }],
        SessionEventKind::ApprovalResponded {
            id,
            decision,
            responder,
            updated_input: _,
        } => vec![AgentEvent::StatusUpdate {
            action: "approval_responded".into(),
            detail: format!(
                "{id} → {decision} (by {})",
                responder.as_deref().unwrap_or("unknown")
            ),
        }],
        SessionEventKind::CheckpointCreated {
            cp_id,
            edit_batch_id,
        } => vec![AgentEvent::StatusUpdate {
            action: "checkpoint_created".into(),
            detail: format!(
                "{cp_id}{}",
                edit_batch_id
                    .as_deref()
                    .map(|b| format!(" ↔ batch {b}"))
                    .unwrap_or_default()
            ),
        }],
        SessionEventKind::OpenFileMarked {
            path,
            cursor,
            source,
        } => vec![AgentEvent::StatusUpdate {
            action: "open_file_marked".into(),
            detail: format!(
                "{path}{} via {source}",
                cursor
                    .map(|(l, c)| format!(" @ {l}:{c}"))
                    .unwrap_or_default()
            ),
        }],
        SessionEventKind::ProviderRetry {
            attempt,
            max_attempts,
            wait_ms,
            class,
            provider,
            model,
            message,
        } => vec![AgentEvent::StatusUpdate {
            action: "provider_retry".into(),
            detail: format!(
                "{class} attempt={attempt}/{max_attempts} wait_ms={wait_ms} provider={provider} model={model}: {message}"
            ),
        }],
        SessionEventKind::WorkerSpawned {
            parent_tool_use_id,
            worker_id,
            title,
            model,
        } => vec![
            AgentEvent::SubagentSpawn {
                id: worker_id.clone(),
                description: title.clone(),
            },
            AgentEvent::SubagentChildEvent {
                agent_id: worker_id.clone(),
                task_id: parent_tool_use_id.clone(),
                block_kind: "Status".into(),
                payload: serde_json::json!({
                    "action": "spawned",
                    "text": format!("'{title}' ({model})"),
                }),
            },
            AgentEvent::StatusUpdate {
                action: "worker_spawned".into(),
                detail: format!("{worker_id} '{title}' ({model})"),
            },
        ],
        SessionEventKind::WorkerStatus {
            worker_id,
            status,
            detail,
        } => {
            let lane_status = match status.as_str() {
                "starting" | "starting_up" => crate::agent::bridge_types::SubagentStatus::StartingUp,
                "completed" | "done" | "success" => {
                    crate::agent::bridge_types::SubagentStatus::Completed
                }
                "failed" | "error" | "stopped" => {
                    crate::agent::bridge_types::SubagentStatus::Failed
                }
                _ => crate::agent::bridge_types::SubagentStatus::Running,
            };
            vec![
                AgentEvent::SubagentUpdate {
                    id: worker_id.clone(),
                    status: lane_status,
                    result: detail.clone(),
                },
                AgentEvent::StatusUpdate {
                    action: "worker_status".into(),
                    detail: format!(
                        "{worker_id} status={status}{}",
                        detail
                            .as_deref()
                            .map(|d| format!(" detail={d}"))
                            .unwrap_or_default()
                    ),
                },
            ]
        }
        SessionEventKind::WorkerProgress {
            worker_id,
            action,
            detail,
        } => vec![
            AgentEvent::SubagentChildEvent {
                agent_id: worker_id.clone(),
                task_id: String::new(),
                block_kind: "Status".into(),
                payload: serde_json::json!({
                    "action": action,
                    "text": detail,
                }),
            },
            AgentEvent::StatusUpdate {
                action: "worker_progress".into(),
                detail: format!("{worker_id} {action}: {detail}"),
            },
        ],
        SessionEventKind::WorkerCompleted {
            worker_id,
            success,
            summary,
        } => vec![
            AgentEvent::SubagentUpdate {
                id: worker_id.clone(),
                status: if *success {
                    crate::agent::bridge_types::SubagentStatus::Completed
                } else {
                    crate::agent::bridge_types::SubagentStatus::Failed
                },
                result: Some(summary.clone()),
            },
            AgentEvent::StatusUpdate {
                action: if *success {
                    "worker_completed".into()
                } else {
                    "worker_failed".into()
                },
                detail: format!("{worker_id}: {summary}"),
            },
        ],
        SessionEventKind::WorkerStopped { worker_id, reason } => vec![
            AgentEvent::SubagentUpdate {
                id: worker_id.clone(),
                status: crate::agent::bridge_types::SubagentStatus::Failed,
                result: Some(reason.clone()),
            },
            AgentEvent::StatusUpdate {
                action: "worker_stopped".into(),
                detail: format!("{worker_id}: {reason}"),
            },
        ],
        SessionEventKind::ParentResumed { reason } => vec![AgentEvent::StatusUpdate {
            action: "parent_resumed".into(),
            detail: reason.clone(),
        }],
    }
}

