// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::providers::ChatMessage;
use crate::util::truncate_with_ellipsis;

pub fn estimate_history_tokens(history: &[ChatMessage]) -> usize {
    history
        .iter()
        .map(|m| m.content.len().div_ceil(4) + 4)
        .sum()
}

pub(crate) fn estimate_tokens_filtered(history: &[ChatMessage], is_system: bool) -> usize {
    history
        .iter()
        .filter(|m| (m.role == "system") == is_system)
        .map(|m| m.content.len().div_ceil(4) + 4)
        .sum()
}

pub(crate) fn build_compaction_transcript(messages: &[ChatMessage], max_chars: usize) -> String {
    let mut transcript = String::new();
    for msg in messages {
        let role = msg.role.to_uppercase();
        let _ = writeln!(transcript, "{role}: {}", msg.content.trim());
    }

    if transcript.chars().count() > max_chars {
        truncate_with_ellipsis(&transcript, max_chars)
    } else {
        transcript
    }
}

pub(crate) fn replace_history_range_with_assistant(
    history: &mut Vec<ChatMessage>,
    start: usize,
    end: usize,
    assistant_content: impl Into<String>,
) {
    history.splice(
        start..end,
        std::iter::once(ChatMessage::assistant(assistant_content.into())),
    );
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InteractiveSessionState {
    pub version: u32,
    pub history: Vec<ChatMessage>,
}

impl InteractiveSessionState {
    pub fn from_history(history: &[ChatMessage]) -> Self {
        Self {
            version: 1,
            history: history.to_vec(),
        }
    }
}

pub(crate) fn load_interactive_session_history(
    path: &Path,
    system_prompt: &str,
) -> Result<Vec<ChatMessage>> {
    if !path.exists() {
        return Ok(vec![ChatMessage::system(system_prompt)]);
    }

    let raw = std::fs::read_to_string(path)?;
    let mut state: InteractiveSessionState = serde_json::from_str(&raw)?;
    if state.history.is_empty() {
        state.history.push(ChatMessage::system(system_prompt));
    } else if state.history.first().map(|msg| msg.role.as_str()) != Some("system") {
        state.history.insert(0, ChatMessage::system(system_prompt));
    }

    Ok(state.history)
}

pub(crate) fn save_interactive_session_history(path: &Path, history: &[ChatMessage]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let payload = serde_json::to_string_pretty(&InteractiveSessionState::from_history(history))?;
    std::fs::write(path, payload)?;
    Ok(())
}
