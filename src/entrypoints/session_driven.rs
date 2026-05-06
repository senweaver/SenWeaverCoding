// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Session-driven CLI shell: an alternative interactive mode that routes
//! all output through `AgentSession` → `render_cli` → stdout.
//!
//! This is the reference thin-shell implementation promised by the
//! architecture plan.  It proves the `AgentSession` event stream is
//! sufficient to run a real conversation with zero UI-specific business
//! logic leaking into the shell itself.
//!
//! # Design
//!
//! 1. Read user input line-by-line from stdin.
//! 2. `session.submit(input)` drives the internal agent loop.
//! 3. A detached task subscribes to `session.subscribe()` and renders
//!    each `SessionEvent` via `render_cli(&event, CliFormat::Pretty)`.
//! 4. Ctrl-C / empty input cleanly exits.
//!
//! Compared to the legacy REPL (`src/agent/run`), no rendering logic
//! appears outside the shared renderer — the shell is 100% event-driven.

use std::io::{self, BufRead, Write};
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

fn provision_actor(session_id: &str) -> Option<Arc<SessionActor>> {
    let root = std::path::PathBuf::from(".sen");
    match SessionEventLog::open_at(&root, session_id) {
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

pub async fn run_session_driven(agent: Arc<Mutex<Agent>>, prompt_label: &str) -> Result<()> {

    let session_id = format!(
        "cli_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let session_actor = provision_actor(&session_id);

    let transcript = Arc::new(parking_lot::Mutex::new(CliTranscriptSink::default()));
    if let Some(ref actor) = session_actor {
        let state = actor.snapshot();
        let _ = replay_state_into_sink(&state, &mut *transcript.lock());
    }
    let _hub_handle = session_actor.as_ref().map(|_| {
        spawn_hub_subscriber(session_id.clone(), transcript.clone(), ChatViewSurface::Cli)
    });

    let (session, event_rx) = match session_actor.clone() {
        Some(actor) => {
            AgentSession::with_agent_and_state(SessionConfig::default(), agent, actor)
        }
        None => AgentSession::with_agent(SessionConfig::default(), agent),
    };
    let session = Arc::new(session);

    let renderer_session = session.clone();
    let renderer = crate::runtime::spawn_supervised("entrypoints.session_renderer", async move {
        let mut rx = event_rx;
        let _ = renderer_session;
        loop {
            match rx.recv().await {
                Ok(event) => {
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
                Err(_) => break,
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

            let mut buf = String::new();
            match reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
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
                Err(_) => break,
            }
        }
    });

    while let Some(input) = input_rx.recv().await {
        session.submit(&input).await;
    }

    drop(session);
    renderer.into_inner().await;
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

    session.submit(input).await;
    drop(session);
    let output = collector_rx.await.unwrap_or_default();
    Ok(output)
}
