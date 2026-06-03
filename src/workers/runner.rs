// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::agent::agent::{Agent, TurnEvent};
use crate::agent::loop_::DraftEvent;
use crate::config::Config;
use crate::session::event::{SessionEvent, SessionEventKind};
use crate::session::turn_event_to_session_event;
use crate::workers::events::{WorkerResult, WorkerSpec, WorkerStatus};
use crate::workers::persistence::{WorkerEventLog, write_meta};
use crate::workers::supervisor::WorkerSupervisor;
use crate::workers::worker::WorkerHandle;

#[derive(Clone)]
pub struct WorkerRunContext {
    pub config: Arc<Config>,

    pub live_config: Option<crate::config::live::LiveConfig>,

    pub parent_workspace_dir: Option<String>,
}

pub async fn run_worker(
    supervisor: Arc<WorkerSupervisor>,
    handle: Arc<WorkerHandle>,
    spec: WorkerSpec,
    parent_draft_tx: Option<mpsc::Sender<DraftEvent>>,
    ctx: WorkerRunContext,
) {
    let workspace_root = supervisor.workspace_root().to_path_buf();

    let event_log = match WorkerEventLog::open(&workspace_root, &handle.worker_id) {
        Ok(log) => Some(Arc::new(log)),
        Err(err) => {
            tracing::warn!(
                worker_id = %handle.worker_id,
                error = %err,
                "failed to open worker event log; running without persistence"
            );
            None
        }
    };

    emit_worker_lifecycle(
        &handle,
        &parent_draft_tx,
        WorkerLifecycle::Spawned,
        event_log.as_deref(),
    )
    .await;

    let mut config_for_agent: Config = (*ctx.config).clone();
    if let Some(ref model) = spec.model {
        if !model.trim().is_empty() {
            config_for_agent.default_model = Some(model.clone());
        }
    }

    let denied = Some(vec!["spawn_workers".to_string()]);
    let live_cfg = ctx.live_config.clone();

    let mut agent = match Agent::from_config(&config_for_agent, denied, live_cfg).await {
        Ok(a) => a,
        Err(err) => {
            let msg = format!("Worker '{}' failed to initialise agent: {err}", spec.title);
            tracing::warn!(worker_id = %handle.worker_id, error = %err, "worker init failed");
            finalize_worker(
                &supervisor,
                &handle,
                &spec,
                &parent_draft_tx,
                event_log.as_deref(),
                WorkerStatus::Failed,
                String::new(),
                Some(msg),
            )
            .await;
            return;
        }
    };

    agent.set_memory_session_id(Some(handle.worker_id.clone()));

    if let Some(ref dir) = ctx.parent_workspace_dir {
        if !dir.trim().is_empty() {
            agent.set_session_workspace_dir(std::path::PathBuf::from(dir));
        }
    }

    handle.set_status(WorkerStatus::Running);
    emit_worker_lifecycle(
        &handle,
        &parent_draft_tx,
        WorkerLifecycle::StatusChanged,
        event_log.as_deref(),
    )
    .await;

    write_meta_safe(&workspace_root, &handle, &spec);

    let (tx, mut rx) = mpsc::channel::<TurnEvent>(128);

    let handle_for_bridge = Arc::clone(&handle);
    let parent_draft_for_bridge = parent_draft_tx.clone();
    let event_log_for_bridge = event_log.clone();
    let bridge = crate::runtime::spawn_supervised("worker.event_bridge", async move {
        while let Some(turn_event) = rx.recv().await {
            forward_turn_event_to_worker_session(
                &handle_for_bridge,
                event_log_for_bridge.as_deref(),
                &turn_event,
            );

            forward_turn_event_to_parent_summary(
                &handle_for_bridge,
                parent_draft_for_bridge.as_ref(),
                &turn_event,
            )
            .await;
        }
    })
    .into_inner();

    let prompt = if let Some(ref ctx_text) = spec.context {
        if ctx_text.trim().is_empty() {
            spec.prompt.clone()
        } else {
            format!("Context:\n{}\n\nTask:\n{}", ctx_text, spec.prompt)
        }
    } else {
        spec.prompt.clone()
    };

    let cancel_for_run = handle.cancel.clone();

    let run_future = agent.turn_streamed(&prompt, tx);

    let result = tokio::select! {
        biased;
        _ = cancel_for_run.cancelled() => Err("worker cancelled by user".to_string()),
        outcome = run_future => outcome.map_err(|e| e.to_string()),
    };

    bridge.abort();

    let (status, output, error_text) = match result {
        Ok(text) => (WorkerStatus::Completed, text, None),
        Err(msg) if handle.is_cancelled() => (
            WorkerStatus::Stopped,
            "Worker was stopped before completion.".to_string(),
            Some(msg),
        ),
        Err(msg) => (WorkerStatus::Failed, String::new(), Some(msg)),
    };

    finalize_worker(
        &supervisor,
        &handle,
        &spec,
        &parent_draft_tx,
        event_log.as_deref(),
        status,
        output,
        error_text,
    )
    .await;
}

fn write_meta_safe(
    workspace_root: &std::path::Path,
    handle: &WorkerHandle,
    spec: &WorkerSpec,
) {
    let meta = handle.to_meta(&spec.prompt, spec.context.as_deref());
    if let Err(err) = write_meta(workspace_root, &meta) {
        tracing::warn!(
            worker_id = %handle.worker_id,
            error = %err,
            "failed to persist worker meta"
        );
    }
}

async fn finalize_worker(
    supervisor: &WorkerSupervisor,
    handle: &Arc<WorkerHandle>,
    spec: &WorkerSpec,
    parent_draft_tx: &Option<mpsc::Sender<DraftEvent>>,
    event_log: Option<&WorkerEventLog>,
    status: WorkerStatus,
    output: String,
    error_text: Option<String>,
) {
    handle.set_status(status);
    handle.mark_finished_now();

    let result = WorkerResult {
        worker_id: handle.worker_id.clone(),
        title: handle.title.clone(),
        status,
        output: output.clone(),
        error: error_text.clone(),
        started_at: handle.started_at,
        finished_at: handle.finished_at(),
    };

    let summary_kind = match status {
        WorkerStatus::Completed => WorkerLifecycle::Completed {
            success: true,
            summary: output.clone(),
        },
        WorkerStatus::Failed => WorkerLifecycle::Completed {
            success: false,
            summary: error_text.clone().unwrap_or_else(|| "failed".to_string()),
        },
        WorkerStatus::Stopped => WorkerLifecycle::Stopped {
            reason: error_text.clone().unwrap_or_else(|| "cancelled".to_string()),
        },
        WorkerStatus::Pending | WorkerStatus::Running => WorkerLifecycle::StatusChanged,
    };

    emit_worker_lifecycle(handle, parent_draft_tx, summary_kind, event_log).await;

    write_meta_safe(supervisor.workspace_root(), handle, spec);

    handle.complete(result);
}

enum WorkerLifecycle {
    Spawned,
    StatusChanged,
    Completed { success: bool, summary: String },
    Stopped { reason: String },
}

async fn emit_worker_lifecycle(
    handle: &Arc<WorkerHandle>,
    parent_draft_tx: &Option<mpsc::Sender<DraftEvent>>,
    kind: WorkerLifecycle,
    event_log: Option<&WorkerEventLog>,
) {
    let event = match &kind {
        WorkerLifecycle::Spawned => DraftEvent::WorkerSpawned {
            parent_tool_use_id: handle.parent_tool_use_id.clone(),
            worker_id: handle.worker_id.clone(),
            title: handle.title.clone(),
            model: handle.model.clone(),
        },
        WorkerLifecycle::StatusChanged => DraftEvent::WorkerStatus {
            worker_id: handle.worker_id.clone(),
            status: handle.status().as_str().to_string(),
            detail: handle.last_detail(),
        },
        WorkerLifecycle::Completed { success, summary } => DraftEvent::WorkerCompleted {
            worker_id: handle.worker_id.clone(),
            success: *success,
            summary: summary.clone(),
        },
        WorkerLifecycle::Stopped { reason } => DraftEvent::WorkerStopped {
            worker_id: handle.worker_id.clone(),
            reason: reason.clone(),
        },
    };

    if let Some(tx) = parent_draft_tx.as_ref() {
        let _ = tx.send(event).await;
    }

    let sess_event = lifecycle_to_session_event(handle, &kind);
    if let Some(log) = event_log {
        if let Err(err) = log.append(&sess_event) {
            tracing::debug!(
                worker_id = %handle.worker_id,
                error = %err,
                "failed to persist worker lifecycle event"
            );
        }
    }
}

fn lifecycle_to_session_event(handle: &WorkerHandle, kind: &WorkerLifecycle) -> SessionEvent {
    match kind {
        WorkerLifecycle::Spawned => SessionEvent::new(SessionEventKind::TurnStarted {
            input: format!("[worker:{}] {}", handle.title, handle.worker_id),
        }),
        WorkerLifecycle::StatusChanged => SessionEvent::new(SessionEventKind::Delta {
            text: format!(
                "[worker:{}] status={}",
                handle.title,
                handle.status().as_str()
            ),
        }),
        WorkerLifecycle::Completed { success, summary } => {
            SessionEvent::new(SessionEventKind::TurnFinished {
                output: if *success {
                    summary.clone()
                } else {
                    format!("[failed] {summary}")
                },
                tokens_used: 0,
            })
        }
        WorkerLifecycle::Stopped { reason } => SessionEvent::new(SessionEventKind::TurnFinished {
            output: format!("[stopped] {reason}"),
            tokens_used: 0,
        }),
    }
}

fn forward_turn_event_to_worker_session(
    handle: &WorkerHandle,
    event_log: Option<&WorkerEventLog>,
    event: &TurnEvent,
) {
    handle.publish_event(event.clone());

    if let Some(sess_event) = turn_event_to_session_event(event.clone()) {
        if let Some(log) = event_log {
            if let Err(err) = log.append(&sess_event) {
                tracing::debug!(
                    worker_id = %handle.worker_id,
                    error = %err,
                    "failed to persist worker session event"
                );
            }
        }
    }

    match event {
        TurnEvent::ToolCall { name, .. } => {
            handle.update_action(Some(name.clone()), None);
        }
        TurnEvent::StatusUpdate { action, detail } => {
            handle.update_action(Some(action.clone()), Some(detail.clone()));
        }
        TurnEvent::Thinking { delta } => {
            if !delta.trim().is_empty() {
                handle.update_action(Some("thinking".to_string()), Some(truncate(delta, 80)));
            }
        }
        TurnEvent::Chunk { delta } => {
            if !delta.trim().is_empty() {
                handle.update_action(Some("writing".to_string()), Some(truncate(delta, 80)));
            }
        }
        _ => {}
    }
}

async fn forward_turn_event_to_parent_summary(
    handle: &WorkerHandle,
    parent_draft_tx: Option<&mpsc::Sender<DraftEvent>>,
    event: &TurnEvent,
) {
    let Some(tx) = parent_draft_tx else {
        return;
    };

    let progress = match event {
        TurnEvent::ToolCall { name, .. } => Some(DraftEvent::WorkerProgress {
            worker_id: handle.worker_id.clone(),
            action: "tool".to_string(),
            detail: name.clone(),
        }),
        TurnEvent::StatusUpdate { action, detail } => Some(DraftEvent::WorkerProgress {
            worker_id: handle.worker_id.clone(),
            action: action.clone(),
            detail: detail.clone(),
        }),
        TurnEvent::Thinking { delta } if !delta.trim().is_empty() => {
            Some(DraftEvent::WorkerProgress {
                worker_id: handle.worker_id.clone(),
                action: "thinking".to_string(),
                detail: truncate(delta, 80),
            })
        }
        TurnEvent::Chunk { delta } if !delta.trim().is_empty() => {
            Some(DraftEvent::WorkerProgress {
                worker_id: handle.worker_id.clone(),
                action: "writing".to_string(),
                detail: truncate(delta, 80),
            })
        }
        _ => None,
    };

    if let Some(evt) = progress {
        let _ = tx.send(evt).await;
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}
