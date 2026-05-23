// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use crate::config::Config;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

pub use crate::agent::bridge_types::{AgentEvent, UserInput};

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
                AgentEvent::Thinking | AgentEvent::ThinkingChunk(_) => self.is_busy = true,
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

async fn run_via_session(
    agent: Arc<Mutex<crate::agent::Agent>>,
    message: String,
    agent_tx: &mpsc::Sender<AgentEvent>,
    session_state: Option<Arc<crate::session::SessionActor>>,
) {
    use crate::session::{AgentSession, SessionConfig, session_to_agent_events};
    use tokio::sync::broadcast::error::RecvError;

    let (session, mut session_rx) = match session_state {
        Some(state) => AgentSession::with_agent_and_state(SessionConfig::default(), agent, state),
        None => AgentSession::with_agent(SessionConfig::default(), agent),
    };

    let session_arc = Arc::new(session);
    let submit_handle = {
        let s = session_arc.clone();
        crate::runtime::spawn_supervised("tui.agent_bridge.submit", async move {
            s.submit(&message).await;
        })
        .into_inner()
    };

    loop {
        match session_rx.recv().await {
            Ok(event) => {
                for agent_event in session_to_agent_events(&event) {
                    if agent_tx.send(agent_event).await.is_err() {

                        return;
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
}

pub fn spawn_agent_task(config: Config) -> AgentBridge {
    let (user_tx, mut user_rx) = mpsc::channel::<UserInput>(32);
    let (agent_tx, agent_rx) = mpsc::channel::<AgentEvent>(128);

    let session_actor_slot: std::sync::Arc<
        once_cell::sync::OnceCell<std::sync::Arc<crate::session::SessionActor>>,
    > = std::sync::Arc::new(once_cell::sync::OnceCell::new());
    let slot_for_task = session_actor_slot.clone();

    let _ = crate::runtime::spawn_supervised("tui.agent_bridge.agent_loop", async move {

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
            let log_root = std::path::PathBuf::from(".sen");
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
                        Vec::<crate::tui::ChatMessage>::new(),
                    ));
                    let handle = crate::session::spawn_hub_subscriber(
                        session_id.clone(),
                        mirror.clone(),
                        crate::session::ChatViewSurface::Tui,
                    );
                    (Some(actor), Some(handle))
                }
                None => (None, None),
            }
        };

        while let Some(input) = user_rx.recv().await {
            match input {
                UserInput::Chat(message) => {
                    let _ = agent_tx.send(AgentEvent::Thinking).await;

                    if let Some(ref shared) = agent_shared {

                        run_via_session(
                            shared.clone(),
                            message,
                            &agent_tx,
                            session_actor.clone(),
                        )
                        .await;
                    } else {

                        let result = Box::pin(crate::agent::run(
                            config.clone(),
                            Some(message),
                            None,
                            None,
                            config.default_temperature,
                            Vec::new(),
                            false,
                            None,
                            None,
                            None,
                        ))
                        .await;
                        match result {
                            Ok(response) => {
                                let _ = agent_tx.send(AgentEvent::AssistantMessage(response)).await;
                            }
                            Err(e) => {
                                let _ = agent_tx.send(AgentEvent::Error(format!("{e:#}"))).await;
                            }
                        }
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::SlashCommand { name, args } => {
                    let _ = agent_tx.send(AgentEvent::Thinking).await;
                    let output = execute_slash_command(&name, &args, &config).await;
                    let _ = agent_tx.send(AgentEvent::CommandOutput(output)).await;
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::ModeSwitch(mode_name) => {
                    let output = execute_slash_command("mode", &[mode_name.clone()], &config).await;
                    let _ = agent_tx.send(AgentEvent::ModeChanged(mode_name)).await;
                    let _ = agent_tx.send(AgentEvent::CommandOutput(output)).await;
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::Cancel => {
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::ClearAndSeedHistory { messages } => {

                    if let Some(ref shared) = agent_shared {
                        shared.lock().await.seed_history(&messages);
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::ReloadAgent => {
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

                    let status = if approved { "approved" } else { "denied" };
                    tracing::info!("TUI: tool {tool_id} {status}");
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::ExecutePlan { plan_content } => {
                    let _ = agent_tx.send(AgentEvent::Thinking).await;
                    if let Some(ref shared) = agent_shared {
                        let exec_msg =
                            format!("Execute the following plan step by step:\n\n{plan_content}");

                        run_via_session(
                            shared.clone(),
                            exec_msg,
                            &agent_tx,
                            session_actor.clone(),
                        )
                        .await;
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::QuestionAnswer { .. } => {

                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::QuestionAnswerBatch { .. } => {

                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::ResumePlan { plan_content } => {
                    let _ = agent_tx.send(AgentEvent::Thinking).await;
                    if let Some(ref shared) = agent_shared {
                        let exec_msg = format!(
                            "Resume the following plan from where it stopped:\n\n{plan_content}"
                        );
                        run_via_session(
                            shared.clone(),
                            exec_msg,
                            &agent_tx,
                            session_actor.clone(),
                        )
                        .await;
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
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
        format!("Services not initialized — cannot run /{name}")
    }
}
