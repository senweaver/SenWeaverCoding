// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `/session` command — showcase the `AgentSession` + shell renderer
//! integration in the currently-running CLI.
//!
//! The main interactive loop still uses `Agent::turn` directly for
//! historical reasons; this command demonstrates the alternative
//! event-stream driven path by wiring a short demonstration turn end
//! to end:
//!
//! 1. Create an `AgentSession`.
//! 2. Spawn a task that emits synthetic `SessionEvent`s mirroring what
//!    a real turn would produce.
//! 3. Consume the event stream and render via `session::shell`.
//!
//! This is not just docs — running `/session demo` exercises the real
//! `AgentSession::subscribe` / `SessionEventSink` / `render_cli`
//! pipeline and surfaces any wiring bugs immediately.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "session",
    aliases: &[],
    description: "Demo or manage AgentSession event streams",
    usage: "/session <demo|status>",
    category: CommandCategory::Debug,
    hidden: false,
    requires_interactive: true,
    remote_safe: false,
    handler: make_handler!(handle_session),
});

pub async fn handle_session(ctx: CommandContext) -> CommandResult {
    match ctx.args.first().map(String::as_str) {
        Some("demo") | None => demo().await,
        Some("status") => status(),
        Some(other) => CommandResult::err(format!(
            "Unknown session subcommand '{other}'. Use: demo | status"
        )),
    }
}

fn status() -> CommandResult {

    let msg: String = "AgentSession infrastructure: ready\n\
         Shell renderers: cli (Pretty/Ndjson/Plain), tui, gui\n\
         Try `/session demo` to see a full event-stream demo."
        .to_string();
    CommandResult::ok(msg)
}

async fn demo() -> CommandResult {
    use crate::session::{AgentSession, CliFormat, SessionConfig, SessionEventKind, render_cli};

    let (session, mut rx) = AgentSession::new(SessionConfig::default());
    let sink = session.sink();

    crate::runtime::spawn_supervised("cli.session.event_producer", async move {
        sink.emit_delta("Starting analysis...\n");
        sink.emit_tool_call(
            "file_read",
            "call_demo_1",
            serde_json::json!({"path": "README.md"}),
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        sink.emit_tool_result("call_demo_1", "# SenWeaverCoding\n...", false);
        sink.emit_delta("Analysis complete.");
    });

    crate::runtime::spawn_supervised("cli.session.submit_turn", async move {
        session.submit("demo input").await;
    });

    let mut output = String::new();
    output.push_str("── AgentSession demo (CliFormat::Pretty) ──\n");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let ev = match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => ev,
            _ => break,
        };
        let (text, newline) = render_cli(&ev, CliFormat::Pretty);
        output.push_str(&text);
        if newline {
            output.push('\n');
        }
        if matches!(ev.kind, SessionEventKind::TurnFinished { .. }) {
            break;
        }
    }
    output.push_str("── end of demo ──");
    CommandResult::ok(output)
}
