// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shell adapters translating `SessionEvent` streams into UI-specific output.
//!
//! The three adapters here represent the "薄壳化" promise: each shell
//! consumes the same typed event stream and is responsible **only** for
//! rendering.  No business logic leaks into `cli_shell` / `tui_shell` /
//! `gui_shell`.
//!
//! Adapters are intentionally minimal in this iteration — they expose a
//! pure `render(event) -> String | Vec<u8>` API that downstream shells
//! compose into their native I/O loops.

use super::event::{SessionEvent, SessionEventKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliFormat {

    Pretty,

    Ndjson,

    Plain,
}

pub fn render_cli(event: &SessionEvent, fmt: CliFormat) -> (String, bool) {
    match fmt {
        CliFormat::Ndjson => (
            serde_json::to_string(event).unwrap_or_else(|_| "{}".into()),
            true,
        ),
        CliFormat::Pretty => render_cli_pretty(&event.kind),
        CliFormat::Plain => render_cli_plain(&event.kind),
    }
}

fn render_cli_pretty(kind: &SessionEventKind) -> (String, bool) {
    match kind {
        SessionEventKind::TurnStarted { input } => {
            (format!("▶ turn started: {}", truncate(input, 80)), true)
        }
        SessionEventKind::Delta { text } => (text.clone(), false),
        SessionEventKind::ToolCall {
            tool_name,
            tool_call_id,
            arguments,
        } => (
            format!(
                "⚒ {tool_name}({})  [id={tool_call_id}]",
                truncate(&arguments.to_string(), 60)
            ),
            true,
        ),
        SessionEventKind::ToolResult {
            tool_call_id,
            output,
            is_error,
        } => {
            let marker = if *is_error { "✗" } else { "✓" };
            (
                format!("{marker} [id={tool_call_id}] {}", truncate(output, 200)),
                true,
            )
        }
        SessionEventKind::TurnFinished {
            output,
            tokens_used,
        } => (
            format!(
                "◀ turn finished ({tokens_used} tokens): {}",
                truncate(output, 80)
            ),
            true,
        ),
        SessionEventKind::Error { message } => (format!("✗ error: {message}"), true),
        SessionEventKind::ContextCompressed {
            tokens_before,
            tokens_after,
        } => (
            format!("… context compressed {tokens_before} → {tokens_after} tokens"),
            true,
        ),
        SessionEventKind::ModeChanged { mode } => (format!("• mode changed: {mode}"), true),
        SessionEventKind::FirstToken {
            agent_id,
            elapsed_ms,
        } => (format!("✦ first token ({agent_id}) {elapsed_ms} ms"), true),
        SessionEventKind::WritePlanCreated {
            goal,
            summary,
            steps,
        } => (
            format!(
                "◉ write plan ({steps} steps) — {} [{summary}]",
                truncate(goal, 60)
            ),
            true,
        ),
        SessionEventKind::WriteStepStarted { index, label } => {
            (format!("→ step {index} {label} …"), true)
        }
        SessionEventKind::WriteStepFinished {
            index,
            label,
            ok,
            summary,
        } => (
            format!(
                "{} step {index} {label}  {}",
                if *ok { "✓" } else { "✗" },
                truncate(summary, 100)
            ),
            true,
        ),
        SessionEventKind::WriteVerify { status } => (format!("◆ verify: {status}"), true),
        SessionEventKind::DiffSessionApplied {
            files,
            hunks_exact,
            hunks_fuzzy,
        } => (
            format!(
                "✚ diff batch applied: {files} files ({hunks_exact} exact, {hunks_fuzzy} fuzzy)"
            ),
            true,
        ),
        SessionEventKind::DiffSessionRolledBack { files } => {
            (format!("⎌ diff batch rolled back ({files} files)"), true)
        }
        SessionEventKind::ApprovalRequested {
            id, tool_name, ..
        } => (
            format!("? approval requested: {tool_name} [id={id}]"),
            true,
        ),
        SessionEventKind::ApprovalResponded {
            id,
            decision,
            responder,
            updated_input: _,
        } => {
            let who = responder.as_deref().unwrap_or("unknown");
            (
                format!("• approval {id} → {decision} (by {who})"),
                true,
            )
        }
        SessionEventKind::CheckpointCreated {
            cp_id,
            edit_batch_id,
        } => {
            let suffix = edit_batch_id
                .as_ref()
                .map(|b| format!(" ↔ batch {b}"))
                .unwrap_or_default();
            (format!("◈ checkpoint {cp_id}{suffix}"), true)
        }
        SessionEventKind::OpenFileMarked {
            path,
            cursor,
            source,
        } => {
            let hint = cursor
                .map(|(l, c)| format!(" @ {l}:{c}"))
                .unwrap_or_default();
            (format!("◈ opened {path}{hint} via {source}"), true)
        }
    }
}

fn render_cli_plain(kind: &SessionEventKind) -> (String, bool) {
    match kind {
        SessionEventKind::Delta { text } => (text.clone(), false),
        SessionEventKind::ToolCall { tool_name, .. } => (format!("[tool_call {tool_name}]"), true),
        SessionEventKind::ToolResult { is_error, .. } => {
            (format!("[tool_result is_error={is_error}]"), true)
        }
        SessionEventKind::TurnStarted { .. } => ("[turn_started]".to_string(), true),
        SessionEventKind::TurnFinished { .. } => ("[turn_finished]".to_string(), true),
        SessionEventKind::Error { message } => (format!("[error] {message}"), true),
        SessionEventKind::ContextCompressed { .. } => ("[context_compressed]".to_string(), true),
        SessionEventKind::ModeChanged { mode } => (format!("[mode] {mode}"), true),
        SessionEventKind::FirstToken {
            agent_id,
            elapsed_ms,
        } => (
            format!("[first_token agent={agent_id} elapsed_ms={elapsed_ms}]"),
            true,
        ),
        SessionEventKind::WritePlanCreated { steps, .. } => {
            (format!("[write_plan steps={steps}]"), true)
        }
        SessionEventKind::WriteStepStarted { index, label } => (
            format!("[write_step_start index={index} label={label}]"),
            true,
        ),
        SessionEventKind::WriteStepFinished {
            index, label, ok, ..
        } => (
            format!("[write_step_end index={index} label={label} ok={ok}]"),
            true,
        ),
        SessionEventKind::WriteVerify { status } => {
            (format!("[write_verify status={status}]"), true)
        }
        SessionEventKind::DiffSessionApplied {
            files,
            hunks_exact,
            hunks_fuzzy,
        } => (
            format!("[diff_session_applied files={files} exact={hunks_exact} fuzzy={hunks_fuzzy}]"),
            true,
        ),
        SessionEventKind::DiffSessionRolledBack { files } => {
            (format!("[diff_session_rolled_back files={files}]"), true)
        }
        SessionEventKind::ApprovalRequested {
            id, tool_name, ..
        } => (
            format!("[approval_requested id={id} tool={tool_name}]"),
            true,
        ),
        SessionEventKind::ApprovalResponded {
            id, decision, ..
        } => (
            format!("[approval_responded id={id} decision={decision}]"),
            true,
        ),
        SessionEventKind::CheckpointCreated {
            cp_id,
            edit_batch_id,
        } => (
            format!(
                "[checkpoint_created cp={cp_id} batch={}]",
                edit_batch_id.as_deref().unwrap_or("-")
            ),
            true,
        ),
        SessionEventKind::OpenFileMarked {
            path,
            cursor,
            source,
        } => (
            format!(
                "[open_file_marked path={path} cursor={} source={source}]",
                cursor
                    .map(|(l, c)| format!("{l}:{c}"))
                    .unwrap_or_else(|| "-".into())
            ),
            true,
        ),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.replace('\n', " ");
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out.replace('\n', " ")
}

#[derive(Debug, Clone)]
pub struct TuiLine {
    pub prefix: &'static str,
    pub body: String,
    pub style_hint: TuiStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiStyle {
    Normal,
    Dim,
    Accent,
    Error,
    Success,
}

pub fn render_tui(event: &SessionEvent) -> TuiLine {
    match &event.kind {
        SessionEventKind::TurnStarted { input } => TuiLine {
            prefix: "▶",
            body: input.clone(),
            style_hint: TuiStyle::Accent,
        },
        SessionEventKind::Delta { text } => TuiLine {
            prefix: "",
            body: text.clone(),
            style_hint: TuiStyle::Normal,
        },
        SessionEventKind::ToolCall {
            tool_name,
            arguments,
            ..
        } => TuiLine {
            prefix: "⚒",
            body: format!("{tool_name}({})", truncate(&arguments.to_string(), 80)),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::ToolResult {
            output, is_error, ..
        } => TuiLine {
            prefix: if *is_error { "✗" } else { "✓" },
            body: truncate(output, 200),
            style_hint: if *is_error {
                TuiStyle::Error
            } else {
                TuiStyle::Success
            },
        },
        SessionEventKind::TurnFinished { output, .. } => TuiLine {
            prefix: "◀",
            body: truncate(output, 120),
            style_hint: TuiStyle::Success,
        },
        SessionEventKind::Error { message } => TuiLine {
            prefix: "✗",
            body: message.clone(),
            style_hint: TuiStyle::Error,
        },
        SessionEventKind::ContextCompressed {
            tokens_before,
            tokens_after,
        } => TuiLine {
            prefix: "…",
            body: format!("compressed {tokens_before} → {tokens_after}"),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::ModeChanged { mode } => TuiLine {
            prefix: "•",
            body: format!("mode: {mode}"),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::FirstToken {
            agent_id,
            elapsed_ms,
        } => TuiLine {
            prefix: "✦",
            body: format!("first token ({agent_id}) {elapsed_ms} ms"),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::WritePlanCreated {
            goal,
            summary,
            steps,
        } => TuiLine {
            prefix: "◉",
            body: format!("plan ({steps} steps) {} [{summary}]", truncate(goal, 60)),
            style_hint: TuiStyle::Accent,
        },
        SessionEventKind::WriteStepStarted { index, label } => TuiLine {
            prefix: "→",
            body: format!("step {index} {label} …"),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::WriteStepFinished {
            index,
            label,
            ok,
            summary,
        } => TuiLine {
            prefix: if *ok { "✓" } else { "✗" },
            body: format!("step {index} {label}  {}", truncate(summary, 80)),
            style_hint: if *ok {
                TuiStyle::Success
            } else {
                TuiStyle::Error
            },
        },
        SessionEventKind::WriteVerify { status } => TuiLine {
            prefix: "◆",
            body: format!("verify: {status}"),
            style_hint: TuiStyle::Accent,
        },
        SessionEventKind::DiffSessionApplied {
            files,
            hunks_exact,
            hunks_fuzzy,
        } => TuiLine {
            prefix: "✚",
            body: format!("diff batch applied {files}f / {hunks_exact}e / {hunks_fuzzy}fz"),
            style_hint: TuiStyle::Success,
        },
        SessionEventKind::DiffSessionRolledBack { files } => TuiLine {
            prefix: "⎌",
            body: format!("diff batch rolled back ({files} files)"),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::ApprovalRequested {
            id, tool_name, ..
        } => TuiLine {
            prefix: "?",
            body: format!("approval requested: {tool_name} [id={id}]"),
            style_hint: TuiStyle::Accent,
        },
        SessionEventKind::ApprovalResponded {
            id,
            decision,
            responder,
            updated_input: _,
        } => TuiLine {
            prefix: "•",
            body: format!(
                "approval {id} → {decision} (by {})",
                responder.as_deref().unwrap_or("unknown")
            ),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::CheckpointCreated {
            cp_id,
            edit_batch_id,
        } => TuiLine {
            prefix: "◈",
            body: format!(
                "checkpoint {cp_id}{}",
                edit_batch_id
                    .as_ref()
                    .map(|b| format!(" ↔ batch {b}"))
                    .unwrap_or_default()
            ),
            style_hint: TuiStyle::Dim,
        },
        SessionEventKind::OpenFileMarked {
            path,
            cursor,
            source,
        } => TuiLine {
            prefix: "◎",
            body: format!(
                "opened {path}{} via {source}",
                cursor.map(|(l, c)| format!(" @ {l}:{c}")).unwrap_or_default()
            ),
            style_hint: TuiStyle::Dim,
        },
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GuiEvent {
    pub kind: &'static str,
    pub body: String,
    pub is_error: bool,
}

pub fn render_gui(event: &SessionEvent) -> GuiEvent {
    match &event.kind {
        SessionEventKind::TurnStarted { input } => GuiEvent {
            kind: "turn_started",
            body: input.clone(),
            is_error: false,
        },
        SessionEventKind::Delta { text } => GuiEvent {
            kind: "delta",
            body: text.clone(),
            is_error: false,
        },
        SessionEventKind::ToolCall { tool_name, .. } => GuiEvent {
            kind: "tool_call",
            body: tool_name.clone(),
            is_error: false,
        },
        SessionEventKind::ToolResult {
            output, is_error, ..
        } => GuiEvent {
            kind: "tool_result",
            body: output.clone(),
            is_error: *is_error,
        },
        SessionEventKind::TurnFinished { output, .. } => GuiEvent {
            kind: "turn_finished",
            body: output.clone(),
            is_error: false,
        },
        SessionEventKind::Error { message } => GuiEvent {
            kind: "error",
            body: message.clone(),
            is_error: true,
        },
        SessionEventKind::ContextCompressed {
            tokens_before,
            tokens_after,
        } => GuiEvent {
            kind: "context_compressed",
            body: format!("{tokens_before}→{tokens_after}"),
            is_error: false,
        },
        SessionEventKind::ModeChanged { mode } => GuiEvent {
            kind: "mode_changed",
            body: mode.clone(),
            is_error: false,
        },
        SessionEventKind::FirstToken {
            agent_id,
            elapsed_ms,
        } => GuiEvent {
            kind: "first_token",
            body: format!("{agent_id}:{elapsed_ms}"),
            is_error: false,
        },
        SessionEventKind::WritePlanCreated {
            goal,
            summary,
            steps,
        } => GuiEvent {
            kind: "write_plan_created",
            body: format!("{steps}:{summary}:{goal}"),
            is_error: false,
        },
        SessionEventKind::WriteStepStarted { index, label } => GuiEvent {
            kind: "write_step_started",
            body: format!("{index}:{label}"),
            is_error: false,
        },
        SessionEventKind::WriteStepFinished {
            index,
            label,
            ok,
            summary,
        } => GuiEvent {
            kind: "write_step_finished",
            body: format!("{index}:{label}:{ok}:{summary}"),
            is_error: !ok,
        },
        SessionEventKind::WriteVerify { status } => GuiEvent {
            kind: "write_verify",
            body: status.clone(),
            is_error: status.starts_with("Failed"),
        },
        SessionEventKind::DiffSessionApplied {
            files,
            hunks_exact,
            hunks_fuzzy,
        } => GuiEvent {
            kind: "diff_session_applied",
            body: format!("{files}:{hunks_exact}:{hunks_fuzzy}"),
            is_error: false,
        },
        SessionEventKind::DiffSessionRolledBack { files } => GuiEvent {
            kind: "diff_session_rolled_back",
            body: files.to_string(),
            is_error: false,
        },
        SessionEventKind::ApprovalRequested {
            id, tool_name, ..
        } => GuiEvent {
            kind: "approval_requested",
            body: format!("{id}:{tool_name}"),
            is_error: false,
        },
        SessionEventKind::ApprovalResponded {
            id, decision, ..
        } => GuiEvent {
            kind: "approval_responded",
            body: format!("{id}:{decision}"),
            is_error: decision == "denied",
        },
        SessionEventKind::CheckpointCreated {
            cp_id,
            edit_batch_id,
        } => GuiEvent {
            kind: "checkpoint_created",
            body: format!(
                "{cp_id}:{}",
                edit_batch_id.as_deref().unwrap_or("")
            ),
            is_error: false,
        },
        SessionEventKind::OpenFileMarked {
            path,
            cursor,
            source,
        } => GuiEvent {
            kind: "open_file_marked",
            body: format!(
                "{path}|{}|{source}",
                cursor
                    .map(|(l, c)| format!("{l}:{c}"))
                    .unwrap_or_default()
            ),
            is_error: false,
        },
    }
}
