// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::io::{self, Write};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::agent::agent::Agent;
use crate::session::{
    AgentSession, ChatViewSink, ChatViewSurface, CliFormat, SessionActor, SessionConfig,
    SessionEventKind, SessionEventLog, SessionSyncHub, render_cli, replay_state_into_sink,
    spawn_hub_subscriber,
};

#[derive(Debug, Default, Clone)]
pub struct CliTranscriptSink {
    pub lines: Vec<(String, String)>,
}

impl ChatViewSink for CliTranscriptSink {
    fn push_user(&mut self, text: &str) {
        self.lines.push(("user".into(), text.to_string()));
    }
    fn append_assistant_delta(&mut self, text: &str) {
        if let Some(last) = self.lines.last_mut() {
            if last.0 == "assistant" {
                last.1.push_str(text);
                return;
            }
        }
        self.lines.push(("assistant".into(), text.to_string()));
    }
    fn close_assistant_turn(&mut self, output: &str) {
        if output.is_empty() {
            return;
        }
        if self
            .lines
            .last()
            .map(|l| l.0 == "assistant" && !l.1.is_empty())
            .unwrap_or(false)
        {
            return;
        }
        self.lines.push(("assistant".into(), output.to_string()));
    }
    fn push_tool_call(&mut self, tool_name: &str, arguments: &serde_json::Value) {
        self.lines
            .push(("tool".into(), format!("{tool_name}({arguments})")));
    }
    fn push_tool_result(&mut self, output: &str, is_error: bool) {
        self.lines.push((
            if is_error { "tool_err".into() } else { "tool_result".into() },
            output.to_string(),
        ));
    }
    fn push_error(&mut self, message: &str) {
        self.lines.push(("error".into(), message.to_string()));
    }
    fn push_system(&mut self, message: &str) {
        self.lines.push(("system".into(), message.to_string()));
    }
}

fn provision_actor(
    workspace_root: &std::path::Path,
    session_id: &str,
) -> Option<Arc<SessionActor>> {
    match SessionEventLog::open_at(workspace_root, session_id) {
        Ok(log) => Some(SessionActor::open_or_create(
            session_id.to_string(),
            Arc::new(log),
            SessionSyncHub::global(),
        )),
        Err(err) => {
            tracing::warn!(error = %err, "CLI: failed to open session event log; persistence disabled");
            None
        }
    }
}

pub async fn run_session_driven(
    agent: Arc<Mutex<Agent>>,
    prompt_label: &str,
    initial_prompt: Option<String>,
    resume_session_id: Option<String>,
) -> Result<()> {

    let resuming = resume_session_id.is_some();
    let session_id = resume_session_id.unwrap_or_else(|| {
        format!(
            "cli_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )
    });

    let workspace_root = {
        let guard = agent.lock().await;
        guard.current_workspace_dir().to_path_buf()
    };
    let session_actor = provision_actor(&workspace_root, &session_id);

    let transcript = Arc::new(parking_lot::Mutex::new(CliTranscriptSink::default()));
    if resuming
        && !session_actor
            .as_ref()
            .map(|actor| !actor.snapshot().turns.is_empty())
            .unwrap_or(false)
    {
        let mut stdout = io::stdout().lock();
        let _ = writeln!(
            stdout,
            "Session '{session_id}' has no recorded history under {}; starting a fresh session with this id.",
            workspace_root.display()
        );
        let _ = stdout.flush();
    }
    if let Some(ref actor) = session_actor {
        let state = actor.snapshot();
        let _ = replay_state_into_sink(&state, &mut *transcript.lock());
        if resuming && !state.turns.is_empty() {
            let mut messages: Vec<crate::providers::traits::ChatMessage> =
                Vec::with_capacity(state.turns.len() * 2);
            for turn in &state.turns {
                messages.push(crate::providers::traits::ChatMessage::user(
                    turn.input.clone(),
                ));
                if let Some(output) = turn.output.as_ref() {
                    if !output.is_empty() {
                        messages.push(crate::providers::traits::ChatMessage::assistant(
                            output.clone(),
                        ));
                    }
                }
            }
            agent.lock().await.seed_history(&messages);
            let mut stdout = io::stdout().lock();
            let _ = writeln!(
                stdout,
                "Resumed session {session_id} ({} turns restored)",
                state.turns.len()
            );
            let _ = stdout.flush();
        }
    }
    let _hub_handle = session_actor.as_ref().map(|_| {
        spawn_hub_subscriber(session_id.clone(), transcript.clone(), ChatViewSurface::Cli)
    });

    let session_ctx = {
        let workspace_dir = workspace_root.to_string_lossy().into_owned();
        crate::session::SessionContext {
            session_id: session_id.clone(),
            workspace_key: workspace_dir.clone(),
            title: session_id.clone(),
            workspace_dir,
            connection_id: None,
        }
    };

    let cli_permission_mode = crate::gateway::ws::desktop::desktop_runtime_state()
        .permission_mode_for(&session_id);

    let (agent_cancel_flag, agent_cancel_signal) = {
        let guard = agent.lock().await;
        (guard.cancel_token(), guard.cancel_signal_handle())
    };

    let (session, event_rx) = match session_actor.clone() {
        Some(actor) => {
            AgentSession::with_agent_and_state(SessionConfig::default(), agent, actor)
        }
        None => AgentSession::with_agent(SessionConfig::default(), agent),
    };
    let session = Arc::new(session);

    let turn_active = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown = tokio_util::sync::CancellationToken::new();
    let cancel_presses = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let force_abort_slot: Arc<
        parking_lot::Mutex<Option<tokio_util::sync::CancellationToken>>,
    > = Arc::new(parking_lot::Mutex::new(None));
    let _ctrl_c_task = {
        let turn_active = turn_active.clone();
        let shutdown = shutdown.clone();
        let cancel_flag = agent_cancel_flag.clone();
        let cancel_signal = agent_cancel_signal.clone();
        let cancel_presses = cancel_presses.clone();
        let force_abort_slot = force_abort_slot.clone();
        crate::runtime::spawn_supervised("entrypoints.cli_ctrl_c", async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    break;
                }
                if turn_active.load(std::sync::atomic::Ordering::Relaxed) {
                    cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    cancel_signal.load_full().cancel();
                    let presses =
                        cancel_presses.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    let mut stdout = io::stdout().lock();
                    if presses >= 2 {
                        if let Some(tok) = force_abort_slot.lock().clone() {
                            tok.cancel();
                        }
                        let _ = writeln!(
                            stdout,
                            "\n\x1b[31m[force-abort] Force-aborting the current turn\x1b[0m"
                        );
                    } else {
                        let _ = writeln!(
                            stdout,
                            "\n\x1b[33m[cancelled] Cancellation requested for the current turn (press Ctrl+C again to force-abort; press Ctrl+C at the prompt to exit)\x1b[0m"
                        );
                    }
                    let _ = stdout.flush();
                } else {
                    shutdown.cancel();
                    break;
                }
            }
        })
    };

    let renderer_session = session.clone();
    let renderer = crate::runtime::spawn_supervised("entrypoints.session_renderer", async move {
        let mut rx = event_rx;
        let _ = renderer_session;
        while let Ok(event) = rx.recv().await {
            let (text, newline) = render_cli(&event, CliFormat::Pretty);
            let mut stdout = io::stdout().lock();
            if newline {
                let _ = writeln!(stdout, "{text}");
            } else {
                let _ = write!(stdout, "{text}");
                let _ = stdout.flush();
            }

            if matches!(event.kind, SessionEventKind::TurnFinished { .. }) {
                let _ = writeln!(stdout);
            }
        }
    });

    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<String>(4);
    let label = prompt_label.to_string();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let handle = stdin.lock();
        let mut reader = io::BufReader::new(handle);
        loop {

            {
                let mut stdout = io::stdout().lock();
                let _ = write!(stdout, "{label}");
                let _ = stdout.flush();
            }

            match crate::cli::input::read_line_lossy(&mut reader) {
                Ok(None) | Err(_) => break,
                Ok(Some(buf)) => {
                    let trimmed = buf.trim().to_string();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed == "/exit" || trimmed == "/quit" {
                        break;
                    }
                    if input_tx.blocking_send(trimmed).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut pending_initial = initial_prompt.and_then(|p| {
        let trimmed = p.trim().to_string();
        if trimmed.is_empty() { None } else { Some(trimmed) }
    });

    loop {
        let input = match pending_initial.take() {
            Some(first) => {
                let mut stdout = io::stdout().lock();
                let _ = writeln!(stdout, "{first}");
                let _ = stdout.flush();
                drop(stdout);
                first
            }
            None => tokio::select! {
                line = input_rx.recv() => match line {
                    Some(line) => line,
                    None => break,
                },
                () = shutdown.cancelled() => break,
            },
        };
        match crate::commands::dispatch::dispatch_slash_input(&input).await {
            crate::commands::dispatch::SlashOutcome::NotCommand => {
                let perm_scoped = crate::gateway::ws::desktop::scope_permission_mode(
                    cli_permission_mode.clone(),
                    session.submit(&input),
                );
                let turn_force = tokio_util::sync::CancellationToken::new();
                *force_abort_slot.lock() = Some(turn_force.clone());
                cancel_presses.store(0, std::sync::atomic::Ordering::Relaxed);
                turn_active.store(true, std::sync::atomic::Ordering::Relaxed);
                let turn_fut =
                    crate::session::scope_session_context(session_ctx.clone(), perm_scoped);
                let result = tokio::select! {
                    r = turn_fut => Some(r),
                    _ = turn_force.cancelled() => None,
                };
                turn_active.store(false, std::sync::atomic::Ordering::Relaxed);
                *force_abort_slot.lock() = None;
                cancel_presses.store(0, std::sync::atomic::Ordering::Relaxed);
                match result {
                    Some(Ok(())) => {}
                    Some(Err(err)) => {
                        tracing::warn!(error = %err, "CLI: session turn failed");
                        let mut stdout = io::stdout().lock();
                        let _ = writeln!(stdout, "\x1b[31m[error] Turn failed: {err}\x1b[0m");
                        let _ = stdout.flush();
                    }
                    None => {
                        tracing::warn!("CLI: session turn force-aborted by user");
                        let mut stdout = io::stdout().lock();
                        let _ = writeln!(
                            stdout,
                            "\x1b[31m[force-abort] Current turn force-aborted\x1b[0m"
                        );
                        let _ = stdout.flush();
                    }
                }
            }
            crate::commands::dispatch::SlashOutcome::Quit => break,
            crate::commands::dispatch::SlashOutcome::Clear => {
                let mut stdout = io::stdout().lock();
                let _ = write!(stdout, "\x1b[2J\x1b[H");
                let _ = stdout.flush();
            }
            crate::commands::dispatch::SlashOutcome::Handled { success, message } => {
                let mut stdout = io::stdout().lock();
                if success {
                    let _ = writeln!(stdout, "{message}");
                } else {
                    let _ = writeln!(stdout, "\x1b[31m{message}\x1b[0m");
                }
            }
            crate::commands::dispatch::SlashOutcome::Followup { message, prompt } => {
                if let Some(msg) = message {
                    let mut stdout = io::stdout().lock();
                    let _ = writeln!(stdout, "{msg}");
                }
                let perm_scoped = crate::gateway::ws::desktop::scope_permission_mode(
                    cli_permission_mode.clone(),
                    session.submit(&prompt),
                );
                turn_active.store(true, std::sync::atomic::Ordering::Relaxed);
                let result =
                    crate::session::scope_session_context(session_ctx.clone(), perm_scoped).await;
                turn_active.store(false, std::sync::atomic::Ordering::Relaxed);
                if let Err(err) = result {
                    tracing::warn!(error = %err, "CLI: session follow-up turn failed");
                    let mut stdout = io::stdout().lock();
                    let _ = writeln!(stdout, "\x1b[31m[error] Follow-up turn failed: {err}\x1b[0m");
                    let _ = stdout.flush();
                }
            }
        }
    }

    drop(session);
    let _ = renderer.into_inner().await;
    Ok(())
}

pub async fn submit_single_turn(agent: Arc<Mutex<Agent>>, input: &str) -> Result<String> {
    let (session, mut rx) = AgentSession::with_agent(SessionConfig::default(), agent);
    let session = Arc::new(session);

    let (collector_tx, collector_rx) = tokio::sync::oneshot::channel();
    let _collector =
        crate::runtime::spawn_supervised("entrypoints.single_turn_collector", async move {
            let mut buf = String::new();
            let mut saw_finish = false;
            while !saw_finish {
                match rx.recv().await {
                    Ok(event) => {
                        if matches!(event.kind, SessionEventKind::TurnFinished { .. }) {
                            saw_finish = true;
                        }
                        let (text, newline) = render_cli(&event, CliFormat::Pretty);
                        buf.push_str(&text);
                        if newline {
                            buf.push('\n');
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = collector_tx.send(buf);
        });

    let _ = session.submit(input).await;
    drop(session);
    let output = collector_rx.await.map_err(|_| {
        anyhow::anyhow!(
            "single-turn output collector channel closed before the turn finished; no output was captured"
        )
    })?;
    Ok(output)
}
