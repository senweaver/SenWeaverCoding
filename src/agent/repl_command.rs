// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Structured representation of REPL input.  Pure parser, no I/O,
//! no global state — the unit-testable seam pulled out of
//! `loop_::run`'s F-section main loop.
//!
//! ## Why this module exists
//!
//! The interactive REPL inside `pub async fn run` (~635 LOC in
//! [`crate::agent::loop_`]) has historically dispatched user input
//! via a chain of `starts_with("/quit")`, `starts_with("/clear")`,
//! `parse_slash_command_line(...)`, etc. inlined directly into the
//! `loop {}` body.  That made the dispatch logic **untestable**:
//! you had to bring up the entire REPL runtime (stdin, spawn,
//! agent, tool registry) just to confirm `/quit` still terminated
//! the loop.
//!
//! This module extracts the dispatch decision as a pure function so
//! edge cases (`/quit` with trailing whitespace, unknown slash
//! commands, `@file` references at the start, ctrl-D sentinel, …)
//! can be exercised with a constant-time unit test.

use crate::commands::registry::CommandRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplCommand {

    Empty,

    Quit,

    Clear,

    ContinuePlan,

    Slash { name: String, args: Vec<String> },

    Chat { raw: String },
}

impl ReplCommand {

    pub fn is_terminal(&self) -> bool {
        matches!(self, ReplCommand::Quit)
    }

    pub fn is_chat(&self) -> bool {
        matches!(self, ReplCommand::Chat { .. })
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            ReplCommand::Empty => "empty",
            ReplCommand::Quit => "quit",
            ReplCommand::Clear => "clear",
            ReplCommand::ContinuePlan => "continue_plan",
            ReplCommand::Slash { .. } => "slash",
            ReplCommand::Chat { .. } => "chat",
        }
    }
}

pub fn parse_repl_input(raw: &str, registry: &CommandRegistry) -> ReplCommand {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ReplCommand::Empty;
    }

    let lower = trimmed.to_ascii_lowercase();
    if matches!(lower.as_str(), "/quit" | "/exit" | "/q") {
        return ReplCommand::Quit;
    }
    if lower == "/clear" {
        return ReplCommand::Clear;
    }

    if lower == "continue" || lower == "continue plan" {
        return ReplCommand::ContinuePlan;
    }

    if let Some((name, args)) = parse_slash_line(trimmed) {
        if registry.find(&name).is_some() {
            return ReplCommand::Slash { name, args };
        }

    }

    ReplCommand::Chat {
        raw: trimmed.to_string(),
    }
}

fn parse_slash_line(input: &str) -> Option<(String, Vec<String>)> {
    let s = input.trim();
    if !s.starts_with('/') {
        return None;
    }
    let rest = s[1..].trim_start();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let cmd = parts.next()?.to_string();
    let args: Vec<String> = parts.map(String::from).collect();
    Some((cmd, args))
}
