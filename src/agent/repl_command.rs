// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
