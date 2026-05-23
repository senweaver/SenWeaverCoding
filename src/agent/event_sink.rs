// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use tokio::sync::mpsc;

use crate::agent::TurnEvent;
use crate::agent::loop_::DraftEvent;

pub enum EventSink {
    None,
    Draft(mpsc::Sender<DraftEvent>),
    Turn(mpsc::Sender<TurnEvent>),
    Both {
        draft: mpsc::Sender<DraftEvent>,
        turn: mpsc::Sender<TurnEvent>,
    },
}

impl EventSink {
    #[must_use]
    pub fn none() -> Self {
        Self::None
    }

    #[must_use]
    pub fn draft(sender: mpsc::Sender<DraftEvent>) -> Self {
        Self::Draft(sender)
    }

    #[must_use]
    pub fn turn(sender: mpsc::Sender<TurnEvent>) -> Self {
        Self::Turn(sender)
    }

    #[must_use]
    pub fn both(draft: mpsc::Sender<DraftEvent>, turn: mpsc::Sender<TurnEvent>) -> Self {
        Self::Both { draft, turn }
    }

    pub async fn emit_draft(&self, event: DraftEvent) {
        match self {
            Self::None => {}
            Self::Draft(sender) | Self::Both { draft: sender, .. } => {
                if let Err(err) = sender.send(event.clone()).await {
                    tracing::debug!(
                        target: "agent.event_sink",
                        error = %err,
                        "draft event receiver dropped"
                    );
                }
                if let Self::Both { turn, .. } = self {
                    if let Some(turn_event) = draft_to_turn(event) {
                        let _ = turn.send(turn_event).await;
                    }
                }
            }
            Self::Turn(turn_sender) => {
                if let Some(turn_event) = draft_to_turn(event) {
                    let _ = turn_sender.send(turn_event).await;
                }
            }
        }
    }

    pub async fn emit_turn(&self, event: TurnEvent) {
        match self {
            Self::None | Self::Draft(_) => {}
            Self::Turn(sender) | Self::Both { turn: sender, .. } => {
                if let Err(err) = sender.send(event).await {
                    tracing::debug!(
                        target: "agent.event_sink",
                        error = %err,
                        "turn event receiver dropped"
                    );
                }
            }
        }
    }

    #[must_use]
    pub fn draft_sender(&self) -> Option<mpsc::Sender<DraftEvent>> {
        match self {
            Self::Draft(sender) | Self::Both { draft: sender, .. } => Some(sender.clone()),
            _ => None,
        }
    }

    #[must_use]
    pub fn turn_sender(&self) -> Option<mpsc::Sender<TurnEvent>> {
        match self {
            Self::Turn(sender) | Self::Both { turn: sender, .. } => Some(sender.clone()),
            _ => None,
        }
    }
}

#[must_use]
pub fn draft_to_turn(event: DraftEvent) -> Option<TurnEvent> {
    match event {
        DraftEvent::Clear => None,
        DraftEvent::Progress(text) => Some(TurnEvent::StatusUpdate {
            action: "thinking".into(),
            detail: text,
        }),
        DraftEvent::Content(text) => Some(TurnEvent::Chunk { delta: text }),
        DraftEvent::Thinking(text) => Some(TurnEvent::Thinking { delta: text }),
        DraftEvent::ToolCall {
            name,
            args,
            tool_call_id,
        } => Some(TurnEvent::ToolCall {
            name,
            args,
            tool_call_id,
        }),
        DraftEvent::ToolResult {
            name,
            output,
            success,
            tool_call_id,
        } => Some(TurnEvent::ToolResult {
            name,
            output,
            success,
            tool_call_id,
        }),
        DraftEvent::FileEdit {
            path,
            additions,
            deletions,
            diff,
            edit_batch_id,
        } => Some(TurnEvent::FileEdit {
            path,
            additions,
            deletions,
            diff,
            edit_batch_id,
        }),
        DraftEvent::ProgressTick {
            iteration,
            max_iterations,
            tokens_used,
        } => Some(TurnEvent::ProgressTick {
            iteration,
            max_iterations,
            tokens_used,
        }),
        DraftEvent::ContextCompressed {
            tokens_before,
            tokens_after,
        } => Some(TurnEvent::ContextCompressed {
            tokens_before,
            tokens_after,
        }),
        DraftEvent::Cancelling { reason } => Some(TurnEvent::Cancelling { reason }),
        DraftEvent::Error { message } => Some(TurnEvent::Error { message }),
        DraftEvent::UsageUpdate { .. } => None,
        DraftEvent::Subagent {
            task_id,
            agent_id,
            kind,
            delta,
        } => Some(TurnEvent::SubagentChunk {
            task_id,
            agent_id,
            kind,
            delta,
        }),
        DraftEvent::PiiSanitized { report } => Some(TurnEvent::PiiSanitized { report }),
        DraftEvent::ProviderRetry {
            attempt,
            max_attempts,
            wait_ms,
            class,
            provider,
            model,
            message,
        } => Some(TurnEvent::ProviderRetry {
            attempt,
            max_attempts,
            wait_ms,
            class,
            provider,
            model,
            message,
        }),
        DraftEvent::WorkerSpawned {
            parent_tool_use_id,
            worker_id,
            title,
            model,
        } => Some(TurnEvent::WorkerSpawned {
            parent_tool_use_id,
            worker_id,
            title,
            model,
        }),
        DraftEvent::WorkerStatus {
            worker_id,
            status,
            detail,
        } => Some(TurnEvent::WorkerStatus {
            worker_id,
            status,
            detail,
        }),
        DraftEvent::WorkerProgress {
            worker_id,
            action,
            detail,
        } => Some(TurnEvent::WorkerProgress {
            worker_id,
            action,
            detail,
        }),
        DraftEvent::WorkerCompleted {
            worker_id,
            success,
            summary,
        } => Some(TurnEvent::WorkerCompleted {
            worker_id,
            success,
            summary,
        }),
        DraftEvent::WorkerStopped { worker_id, reason } => {
            Some(TurnEvent::WorkerStopped { worker_id, reason })
        }
        DraftEvent::ParentResumed { reason } => Some(TurnEvent::ParentResumed { reason }),
    }
}
