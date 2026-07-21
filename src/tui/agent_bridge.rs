// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::Config;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

pub use crate::agent::bridge_types::{AgentEvent, UserInput};

type CancelControls = (
    Arc<AtomicBool>,
    Arc<arc_swap::ArcSwap<tokio_util::sync::CancellationToken>>,
);

type SubmitAbortSlot = Arc<std::sync::Mutex<Option<tokio::task::AbortHandle>>>;

fn reset_cancel_controls(controls: Option<&CancelControls>) {
    if let Some((flag, signal)) = controls {
        flag.store(false, Ordering::SeqCst);
        if signal.load_full().is_cancelled() {
            signal.store(Arc::new(tokio_util::sync::CancellationToken::new()));
        }
    }
}

fn trigger_cancel_controls(controls: Option<&CancelControls>) {
    if let Some((flag, signal)) = controls {
        flag.store(true, Ordering::SeqCst);
        signal.load_full().cancel();
    }
}

pub struct AgentBridge {
    pub sender: mpsc::Sender<UserInput>,
    pub receiver: mpsc::Receiver<AgentEvent>,
    pub is_busy: bool,

    pub session_actor_slot:
        std::sync::Arc<once_cell::sync::OnceCell<std::sync::Arc<crate::session::SessionActor>>>,
}

impl AgentBridge {

    pub fn poll_events(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.receiver.try_recv() {
            match &ev {
                AgentEvent::Done | AgentEvent::Error(_) => self.is_busy = false,
                AgentEvent::Thinking
                | AgentEvent::ThinkingChunk(_)
                | AgentEvent::StreamChunk(_) => self.is_busy = true,
                _ => {}
            }
            events.push(ev);
        }
        events
    }

    pub fn send(&self, input: UserInput) -> bool {
        self.sender.try_send(input).is_ok()
    }
}

fn question_events_for(event: &AgentEvent) -> Vec<AgentEvent> {
    let AgentEvent::ToolUse {
        name,
        input: Some(raw),
        ..
    } = event
    else {
        return Vec::new();
    };
    if name != "ask_question" && name != "AskQuestion" {
        return Vec::new();
    }
    let Ok(args) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let already_answered = args
        .get("answers")
        .and_then(|v| v.as_object())
        .map(|m| !m.is_empty())
        .unwrap_or(false)
        || args
            .get("skipped")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    if already_answered {
        return Vec::new();
    }
    let Some(questions) = args.get("questions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    questions
        .iter()
        .enumerate()
        .map(|(idx, q)| {
            let question_id = q
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("q-{idx}"));
            let prompt = q
                .get("prompt")
                .or_else(|| q.get("question"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no prompt)")
                .to_string();
            let options = q
                .get("options")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|o| {
                            let label = o.get("label").and_then(|v| v.as_str())?;
                            let id = o.get("id").and_then(|v| v.as_str()).unwrap_or(label);
                            Some(crate::agent::bridge_types::QuestionOption {
                                id: id.to_string(),
                                label: label.to_string(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let allow_multiple = q
                .get("allow_multiple")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            AgentEvent::QuestionAsked {
                question_id,
                prompt,
                options,
                allow_multiple,
            }
        })
        .collect()
}

fn question_answers_to_user_text(
    items: &[crate::agent::bridge_types::QuestionAnswerItem],
) -> String {
    let mut buf = String::from("Here are my answers to your clarifying questions:\n\n");
    let mut answered_any = false;
    for (idx, item) in items.iter().enumerate() {
        let labels: &[String] = if item.selected_labels.is_empty() {
            &item.selected
        } else {
            &item.selected_labels
        };
        let prompt = if item.prompt.is_empty() {
            format!("question {}", idx + 1)
        } else {
            item.prompt.clone()
        };
        if labels.is_empty() {
            buf.push_str(&format!("{}. {prompt}\n   -> (skipped)\n", idx + 1));
        } else {
            answered_any = true;
            buf.push_str(&format!("{}. {prompt}\n", idx + 1));
            for label in labels {
                buf.push_str(&format!("   -> {label}\n"));
            }
        }
    }
    if answered_any {
        buf.push_str(
            "\nProceed using the answers above. Do not ask the same questions again \
             unless my reply is genuinely ambiguous.\n",
        );
    } else {
        buf.push_str(
            "\nI skipped these questions  -  proceed with reasonable defaults and \
             note any assumptions you make.\n",
        );
    }
    buf
}

async fn run_via_session(
    agent: Arc<Mutex<crate::agent::Agent>>,
    message: String,
    agent_tx: &mpsc::Sender<AgentEvent>,
    session_state: Option<Arc<crate::session::SessionActor>>,
    submit_abort_slot: SubmitAbortSlot,
) {
    use crate::session::{AgentSession, SessionConfig, session_to_agent_events};
    use tokio::sync::broadcast::error::RecvError;

    let (session_ctx, turn_coding_mode) = {
        let guard = agent.lock().await;
        let mode = guard.current_coding_mode().unwrap_or_else(|| {
            crate::services::try_get_services()
                .map(|svc| svc.resolve_coding_mode_for(None))
                .unwrap_or_default()
        });
        let workspace_dir = guard
            .current_workspace_dir()
            .to_string_lossy()
            .into_owned();
        let ctx = crate::session::SessionContext {
            session_id: "tui".to_string(),
            workspace_key: workspace_dir.clone(),
            title: "tui".to_string(),
            workspace_dir,
            connection_id: None,
        };
        (ctx, mode)
    };

    let (session, mut session_rx) = match session_state {
        Some(state) => AgentSession::with_agent_and_state(SessionConfig::default(), agent, state),
        None => AgentSession::with_agent(SessionConfig::default(), agent),
    };

    let turn_permission_mode = crate::gateway::ws::desktop::desktop_runtime_state()
        .permission_mode_for("tui");
    let submit_handle = crate::runtime::spawn_supervised("tui.agent_bridge.submit", async move {
        let fut = session.submit(&message);
        let mode_scoped =
            crate::agent::coding_mode::scope_coding_mode(turn_coding_mode, fut);
        let perm_scoped =
            crate::gateway::ws::desktop::scope_permission_mode(turn_permission_mode, mode_scoped);
        let scoped = crate::session::scope_session_context(session_ctx, perm_scoped);
        if let Err(err) = scoped.await {
            tracing::warn!(error = %err, "TUI: session turn failed");
        }
    })
    .into_inner();

    if let Ok(mut slot) = submit_abort_slot.lock() {
        *slot = Some(submit_handle.abort_handle());
    }

    loop {
        match session_rx.recv().await {
            Ok(event) => {
                for agent_event in session_to_agent_events(&event) {
                    let follow_ups = question_events_for(&agent_event);
                    if agent_tx.send(agent_event).await.is_err() {

                        return;
                    }
                    for follow_up in follow_ups {
                        if agent_tx.send(follow_up).await.is_err() {
                            return;
                        }
                    }
                }
            }
            Err(RecvError::Closed) => break,
            Err(RecvError::Lagged(skipped)) => {
                let _ = agent_tx
                    .send(AgentEvent::StatusUpdate {
                        action: "lag".into(),
                        detail: format!("{skipped} events dropped; slow UI"),
                    })
                    .await;
            }
        }
    }

    if let Err(join_err) = submit_handle.await {
        if join_err.is_panic() {
            let _ = agent_tx
                .send(AgentEvent::Error(format!(
                    "agent session panicked: {join_err}"
                )))
                .await;
        }
    }

    if let Ok(mut slot) = submit_abort_slot.lock() {
        *slot = None;
    }
}

fn spawn_turn(
    agent: Arc<Mutex<crate::agent::Agent>>,
    message: String,
    agent_tx: mpsc::Sender<AgentEvent>,
    session_state: Option<Arc<crate::session::SessionActor>>,
    turn_done_tx: mpsc::Sender<u64>,
    done_seq: Arc<std::sync::atomic::AtomicU64>,
    turn_seq: u64,
    submit_abort_slot: SubmitAbortSlot,
) -> tokio::task::JoinHandle<()> {
    crate::runtime::spawn_supervised("tui.agent_bridge.turn", async move {
        run_via_session(agent, message, &agent_tx, session_state, submit_abort_slot).await;
        done_seq.fetch_max(turn_seq, Ordering::SeqCst);
        let _ = agent_tx.send(AgentEvent::Done).await;
        let _ = turn_done_tx.send(turn_seq).await;
    })
    .into_inner()
}

fn spawn_cancel_watchdog(
    agent_tx: mpsc::Sender<AgentEvent>,
    turn_done_tx: mpsc::Sender<u64>,
    done_seq: Arc<std::sync::atomic::AtomicU64>,
    turn_seq: u64,
    abort_handle: tokio::task::AbortHandle,
    submit_abort_slot: SubmitAbortSlot,
) {
    crate::runtime::spawn_supervised("tui.agent_bridge.cancel_watchdog", async move {
        for _ in 0..50u8 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if done_seq.load(Ordering::SeqCst) >= turn_seq {
                return;
            }
        }
        if done_seq.load(Ordering::SeqCst) >= turn_seq {
            return;
        }
        abort_handle.abort();
        if let Some(submit_abort) = submit_abort_slot
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
        {
            submit_abort.abort();
        }
        done_seq.fetch_max(turn_seq, Ordering::SeqCst);
        let _ = agent_tx
            .send(AgentEvent::StatusUpdate {
                action: "cancel_timeout".into(),
                detail: "turn did not stop within 10s after cancellation; aborting the \
                         background task and releasing the UI"
                    .into(),
            })
            .await;
        let _ = agent_tx.send(AgentEvent::Done).await;
        let _ = turn_done_tx.send(turn_seq).await;
    });
}

pub fn spawn_agent_task(config: Config) -> AgentBridge {
    let (user_tx, mut user_rx) = mpsc::channel::<UserInput>(32);
    let (agent_tx, agent_rx) = mpsc::channel::<AgentEvent>(128);

    let session_actor_slot: std::sync::Arc<
        once_cell::sync::OnceCell<std::sync::Arc<crate::session::SessionActor>>,
    > = std::sync::Arc::new(once_cell::sync::OnceCell::new());
    let slot_for_task = session_actor_slot.clone();

    let _ = crate::runtime::spawn_supervised("tui.agent_bridge.agent_loop", async move {

        crate::approval::install_session_surface_approval_manager(
            &config.autonomy,
            config
                .config_path
                .parent()
                .map(|p| p.join("approval_audit.jsonl")),
        );

        let agent_shared: Option<Arc<Mutex<crate::agent::Agent>>> =
            match crate::agent::Agent::from_config(&config, None, None).await {
                Ok(a) => Some(Arc::new(Mutex::new(a))),
                Err(e) => {
                    tracing::warn!("TUI: failed to initialise agent: {e:#}");
                    None
                }
            };

        let (session_actor, _session_mirror_handle) = {
            let session_id = format!(
                "tui_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            let log_root = config.workspace_dir.clone();
            let log = match crate::session::SessionEventLog::open_at(&log_root, &session_id) {
                Ok(l) => Some(std::sync::Arc::new(l)),
                Err(e) => {
                    tracing::warn!("TUI: failed to open session event log: {e:#}");
                    None
                }
            };
            match log {
                Some(log) => {
                    let hub = crate::session::SessionSyncHub::global();
                    let actor =
                        crate::session::SessionActor::open_or_create(session_id.clone(), log, hub);

                    let _ = slot_for_task.set(actor.clone());
                    let mirror = std::sync::Arc::new(parking_lot::Mutex::new(
                        crate::session::chat_view::NullChatViewSink,
                    ));
                    let handle = crate::session::spawn_hub_subscriber(
                        session_id.clone(),
                        mirror,
                        crate::session::ChatViewSurface::Tui,
                    );
                    (Some(actor), Some(handle))
                }
                None => (None, None),
            }
        };

        let cancel_controls: Option<CancelControls> = match agent_shared {
            Some(ref shared) => {
                let guard = shared.lock().await;
                Some((guard.cancel_token(), guard.cancel_signal_handle()))
            }
            None => None,
        };

        let mut active_turn: Option<tokio::task::JoinHandle<()>> = None;
        let active_submit_abort: SubmitAbortSlot = Arc::new(std::sync::Mutex::new(None));
        let (turn_done_tx, mut turn_done_rx) = mpsc::channel::<u64>(8);
        let done_seq: Arc<std::sync::atomic::AtomicU64> =
            Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut turn_seq: u64 = 0;
        let mut queued_answers: Vec<crate::agent::bridge_types::QuestionAnswerItem> = Vec::new();

        loop {
            let input = tokio::select! {
                maybe_input = user_rx.recv() => {
                    match maybe_input {
                        Some(input) => input,
                        None => break,
                    }
                }
                finished_seq = turn_done_rx.recv() => {
                    let Some(finished_seq) = finished_seq else { break };
                    if finished_seq < turn_seq {
                        continue;
                    }
                    active_turn = None;
                    if queued_answers.is_empty() {
                        continue;
                    }
                    let answers = std::mem::take(&mut queued_answers);
                    if let Some(ref shared) = agent_shared {
                        let _ = agent_tx.send(AgentEvent::Thinking).await;
                        let reply = question_answers_to_user_text(&answers);
                        reset_cancel_controls(cancel_controls.as_ref());
                        turn_seq += 1;
                        active_turn = Some(spawn_turn(
                            shared.clone(),
                            reply,
                            agent_tx.clone(),
                            session_actor.clone(),
                            turn_done_tx.clone(),
                            done_seq.clone(),
                            turn_seq,
                            active_submit_abort.clone(),
                        ));
                    }
                    continue;
                }
            };
            let turn_busy = active_turn
                .as_ref()
                .map(|handle| !handle.is_finished())
                .unwrap_or(false);

            let input = match input {
                UserInput::QuestionAnswer {
                    question_id,
                    prompt,
                    selected,
                    selected_labels,
                } => UserInput::QuestionAnswerBatch {
                    answers: vec![crate::agent::bridge_types::QuestionAnswerItem {
                        question_id,
                        prompt,
                        selected,
                        selected_labels,
                    }],
                },
                other => other,
            };

            match input {
                UserInput::Chat(message) => {
                    if turn_busy {
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  press Esc (or Ctrl+C) to cancel the current turn first.".into(),
                            ))
                            .await;
                        continue;
                    }
                    let _ = agent_tx.send(AgentEvent::Thinking).await;

                    if let Some(ref shared) = agent_shared {
                        reset_cancel_controls(cancel_controls.as_ref());
                        turn_seq += 1;
                        active_turn = Some(spawn_turn(
                            shared.clone(),
                            message,
                            agent_tx.clone(),
                            session_actor.clone(),
                            turn_done_tx.clone(),
                            done_seq.clone(),
                            turn_seq,
                            active_submit_abort.clone(),
                        ));
                    } else {
                        let fallback_config = config.clone();
                        let fallback_tx = agent_tx.clone();
                        turn_seq += 1;
                        let fallback_done_tx = turn_done_tx.clone();
                        let fallback_done_seq = done_seq.clone();
                        let fallback_seq = turn_seq;
                        active_turn = Some(
                            crate::runtime::spawn_supervised(
                                "tui.agent_bridge.turn_fallback",
                                async move {
                                    let result = Box::pin(crate::agent::run(
                                        fallback_config.clone(),
                                        Some(message),
                                        None,
                                        None,
                                        fallback_config.default_temperature,
                                        Vec::new(),
                                        false,
                                        None,
                                        None,
                                        None,
                                    ))
                                    .await;
                                    match result {
                                        Ok(response) => {
                                            let _ = fallback_tx
                                                .send(AgentEvent::AssistantMessage(response))
                                                .await;
                                        }
                                        Err(e) => {
                                            let _ = fallback_tx
                                                .send(AgentEvent::Error(format!("{e:#}")))
                                                .await;
                                        }
                                    }
                                    fallback_done_seq.fetch_max(fallback_seq, Ordering::SeqCst);
                                    let _ = fallback_tx.send(AgentEvent::Done).await;
                                    let _ = fallback_done_tx.send(fallback_seq).await;
                                },
                            )
                            .into_inner(),
                        );
                    }
                }

                UserInput::SlashCommand { name, args } => {
                    let runs_without_agent = slash_command_runs_without_agent(&name);
                    if turn_busy && !runs_without_agent {
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  press Esc (or Ctrl+C) to cancel the current turn first.".into(),
                            ))
                            .await;
                        continue;
                    }
                    if !runs_without_agent {
                        let _ = agent_tx.send(AgentEvent::Thinking).await;
                    }
                    let output = execute_slash_command(&name, &args, &config).await;
                    let _ = agent_tx.send(AgentEvent::CommandOutput(output)).await;
                    if !turn_busy {
                        let _ = agent_tx.send(AgentEvent::Done).await;
                    }
                }

                UserInput::ModeSwitch(mode_name) => {
                    let output = execute_slash_command("mode", &[mode_name.clone()], &config).await;
                    let _ = agent_tx.send(AgentEvent::ModeChanged(mode_name)).await;
                    let _ = agent_tx.send(AgentEvent::CommandOutput(output)).await;
                    if !turn_busy {
                        let _ = agent_tx.send(AgentEvent::Done).await;
                    }
                }

                UserInput::Cancel => {
                    if turn_busy {
                        trigger_cancel_controls(cancel_controls.as_ref());
                        let _ = agent_tx
                            .send(AgentEvent::StatusUpdate {
                                action: "cancelling".into(),
                                detail: "user requested cancellation of the current turn".into(),
                            })
                            .await;
                        if let Some(handle) = active_turn.as_ref() {
                            spawn_cancel_watchdog(
                                agent_tx.clone(),
                                turn_done_tx.clone(),
                                done_seq.clone(),
                                turn_seq,
                                handle.abort_handle(),
                                active_submit_abort.clone(),
                            );
                        }
                    } else {
                        let _ = agent_tx.send(AgentEvent::Done).await;
                    }
                }

                UserInput::ClearAndSeedHistory { messages } => {
                    if turn_busy {
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  cancel the current turn before reseeding history.".into(),
                            ))
                            .await;
                        continue;
                    }
                    if let Some(ref shared) = agent_shared {
                        shared.lock().await.seed_history(&messages);
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::ReloadAgent => {
                    if turn_busy {
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  cancel the current turn before reloading.".into(),
                            ))
                            .await;
                        continue;
                    }
                    match crate::agent::Agent::from_config(&config, None, None).await {
                        Ok(a) => {
                            if let Some(ref shared) = agent_shared {
                                *shared.lock().await = a;
                            }
                        }
                        Err(e) => {
                            let _ = agent_tx
                                .send(AgentEvent::Error(format!("Reload failed: {e:#}")))
                                .await;
                        }
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::HotReloadProvider {
                    provider,
                    api_key: _,
                    api_url: _,
                    model,
                } => {
                    if turn_busy {
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  cancel the current turn before switching provider.".into(),
                            ))
                            .await;
                        continue;
                    }
                    let mut new_config = config.clone();
                    new_config.default_provider = Some(provider.clone());
                    new_config.default_model = Some(model.clone());
                    match crate::agent::Agent::from_config(&new_config, None, None).await {
                        Ok(a) => {
                            if let Some(ref shared) = agent_shared {
                                *shared.lock().await = a;
                            }
                            let _ = agent_tx
                                .send(AgentEvent::ModeChanged(format!(
                                    "Provider switched to {provider}/{model}"
                                )))
                                .await;
                        }
                        Err(e) => {
                            let _ = agent_tx
                                .send(AgentEvent::Error(format!("Hot reload failed: {e:#}")))
                                .await;
                        }
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::ApprovalResponse { tool_id, approved } => {
                    let decision = if approved { "yes" } else { "no" };
                    // Record into the reliable-delivery mailbox BEFORE
                    // broadcasting (like the HTTP/desktop responders): a lagged
                    // broadcast waiter falls back to the mailbox, otherwise the
                    // user's verdict is lost and the turn blocks until timeout.
                    crate::approval::record_session_decision_delivery(&tool_id, decision);
                    let _ = crate::approval::drop_pending_gateway_approval(&tool_id);
                    let event = crate::session::SessionEvent::new(
                        crate::session::SessionEventKind::ApprovalResponded {
                            id: tool_id.clone(),
                            decision: decision.to_string(),
                            responder: Some("tui".to_string()),
                            updated_input: None,
                        },
                    );
                    if let Some(actor) = session_actor.as_ref() {
                        let _ = actor.apply(&event);
                    }
                    let _ = crate::gateway::ws::gateway_approval_bus().send(event);
                    tracing::info!(
                        "TUI: approval {tool_id} -> {decision}"
                    );
                    let _ = agent_tx
                        .send(AgentEvent::StatusUpdate {
                            action: "approval_responded".into(),
                            detail: format!("{tool_id} -> {decision} (tui)"),
                        })
                        .await;
                }

                UserInput::ExecutePlan { plan_content } => {
                    if turn_busy {
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  press Esc (or Ctrl+C) to cancel the current turn first.".into(),
                            ))
                            .await;
                        continue;
                    }
                    if let Some(ref shared) = agent_shared {
                        let _ = agent_tx.send(AgentEvent::Thinking).await;
                        let exec_msg =
                            format!("Execute the following plan step by step:\n\n{plan_content}");
                        reset_cancel_controls(cancel_controls.as_ref());
                        turn_seq += 1;
                        active_turn = Some(spawn_turn(
                            shared.clone(),
                            exec_msg,
                            agent_tx.clone(),
                            session_actor.clone(),
                            turn_done_tx.clone(),
                            done_seq.clone(),
                            turn_seq,
                            active_submit_abort.clone(),
                        ));
                    } else {
                        let _ = agent_tx.send(AgentEvent::Done).await;
                    }
                }

                UserInput::QuestionAnswer { .. } => {

                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::QuestionAnswerBatch { answers } => {
                    if answers.is_empty() {
                        let _ = agent_tx.send(AgentEvent::Done).await;
                        continue;
                    }
                    let _ = agent_tx
                        .send(AgentEvent::QuestionAnswered {
                            items: answers.clone(),
                        })
                        .await;
                    if turn_busy {
                        queued_answers.extend(answers);
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  your answers are queued and will be delivered automatically when the current turn finishes.".into(),
                            ))
                            .await;
                        continue;
                    }
                    if let Some(ref shared) = agent_shared {
                        let _ = agent_tx.send(AgentEvent::Thinking).await;
                        let reply = question_answers_to_user_text(&answers);
                        reset_cancel_controls(cancel_controls.as_ref());
                        turn_seq += 1;
                        active_turn = Some(spawn_turn(
                            shared.clone(),
                            reply,
                            agent_tx.clone(),
                            session_actor.clone(),
                            turn_done_tx.clone(),
                            done_seq.clone(),
                            turn_seq,
                            active_submit_abort.clone(),
                        ));
                    } else {
                        let _ = agent_tx.send(AgentEvent::Done).await;
                    }
                }

                UserInput::ResumePlan { plan_content } => {
                    if turn_busy {
                        let _ = agent_tx
                            .send(AgentEvent::CommandOutput(
                                "Agent is busy  -  press Esc (or Ctrl+C) to cancel the current turn first.".into(),
                            ))
                            .await;
                        continue;
                    }
                    if let Some(ref shared) = agent_shared {
                        let _ = agent_tx.send(AgentEvent::Thinking).await;
                        let exec_msg = format!(
                            "Resume the following plan from where it stopped:\n\n{plan_content}"
                        );
                        reset_cancel_controls(cancel_controls.as_ref());
                        turn_seq += 1;
                        active_turn = Some(spawn_turn(
                            shared.clone(),
                            exec_msg,
                            agent_tx.clone(),
                            session_actor.clone(),
                            turn_done_tx.clone(),
                            done_seq.clone(),
                            turn_seq,
                            active_submit_abort.clone(),
                        ));
                    } else {
                        let _ = agent_tx.send(AgentEvent::Done).await;
                    }
                }
                UserInput::CancelSubagent { id } => {
                    tracing::info!("[tui] subagent cancel requested: {id}");
                }
                UserInput::PromoteToBackground { tool_id } => {
                    tracing::info!("[tui] promote-to-background requested: {tool_id}");
                }
                UserInput::KillBackgroundShell { id } => {
                    tracing::info!("[tui] kill background shell requested: {id}");
                }
            }
        }
    });

    AgentBridge {
        sender: user_tx,
        receiver: agent_rx,
        is_busy: false,
        session_actor_slot,
    }
}

fn slash_command_runs_without_agent(name: &str) -> bool {
    const READ_ONLY_COMMANDS: &[&str] = &[
        "help",
        "status",
        "clear",
        "vim",
        "cost",
        "stats",
        "theme",
        "color",
        "history",
        "context",
        "diff",
        "doctor",
        "metrics",
    ];
    let canonical = crate::services::try_get_services()
        .and_then(|svc| svc.command_registry.find(name).map(|cmd| cmd.name.clone()));
    let key = canonical.as_deref().unwrap_or(name);
    READ_ONLY_COMMANDS.contains(&key)
}

async fn execute_slash_command(name: &str, args: &[String], _config: &Config) -> String {
    if let Some(svc) = crate::services::try_get_services() {
        let ctx = crate::commands::registry::CommandContext {
            session_id: "tui".to_string(),
            cwd: std::env::current_dir().unwrap_or_default(),
            args: args.to_vec(),
            raw_input: format!("/{name} {}", args.join(" ")),
            is_interactive: true,
            is_remote: false,
        };
        let result = svc.command_registry.execute(name, ctx).await;
        result.message.unwrap_or_default()
    } else {
        format!("Services not initialized  -  cannot run /{name}")
    }
}
