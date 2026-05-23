// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::providers::ChatMessage;
use crate::util::truncate_with_ellipsis;

#[allow(dead_code)]
pub(crate) const DEFAULT_MAX_HISTORY_MESSAGES: usize = 50;

#[allow(dead_code)]
pub(crate) const COMPACTION_KEEP_RECENT_MESSAGES: usize = 20;

pub(crate) const COMPACTION_MAX_SOURCE_CHARS: usize = 12_000;

#[allow(dead_code)]
pub(crate) const COMPACTION_MAX_SUMMARY_CHARS: usize = 2_000;

pub fn estimate_history_tokens(history: &[ChatMessage]) -> usize {
    history
        .iter()
        .map(|m| m.content.len().div_ceil(4) + 4)
        .sum()
}

#[allow(dead_code)]
pub(crate) fn estimate_tokens_filtered(history: &[ChatMessage], is_system: bool) -> usize {
    history
        .iter()
        .filter(|m| (m.role == "system") == is_system)
        .map(|m| m.content.len().div_ceil(4) + 4)
        .sum()
}

pub(crate) fn trim_history(history: &mut Vec<ChatMessage>, max_history: usize) {
    let has_system = history.first().is_some_and(|m| m.role == "system");
    let non_system_count = if has_system {
        history.len() - 1
    } else {
        history.len()
    };

    if non_system_count <= max_history {
        return;
    }

    let start = usize::from(has_system);
    let to_remove = non_system_count - max_history;
    history.drain(start..start + to_remove);
}

#[allow(dead_code)]
pub(crate) fn build_compaction_transcript(messages: &[ChatMessage]) -> String {
    let mut transcript = String::new();
    for msg in messages {
        let role = msg.role.to_uppercase();
        let _ = writeln!(transcript, "{role}: {}", msg.content.trim());
    }

    if transcript.chars().count() > COMPACTION_MAX_SOURCE_CHARS {
        truncate_with_ellipsis(&transcript, COMPACTION_MAX_SOURCE_CHARS)
    } else {
        transcript
    }
}

#[allow(dead_code)]
pub(crate) fn apply_compaction_summary(
    history: &mut Vec<ChatMessage>,
    start: usize,
    compact_end: usize,
    summary: &str,
) {
    let summary_msg = ChatMessage::assistant(format!("[Compaction summary]\n{}", summary.trim()));
    history.splice(start..compact_end, std::iter::once(summary_msg));
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
