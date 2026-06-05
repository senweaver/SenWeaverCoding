// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::providers::ChatMessage;
use crate::util::truncate_with_ellipsis;

const MAX_SESSION_HISTORY_BYTES: u64 = 256 * 1024 * 1024;

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

    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_SESSION_HISTORY_BYTES {
            anyhow::bail!(
                "session history file too large ({} bytes, limit {})",
                meta.len(),
                MAX_SESSION_HISTORY_BYTES
            );
        }
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
    crate::util::atomic_write(path, payload.as_bytes())?;
    Ok(())
}

pub(crate) async fn load_interactive_session_history_async(
    path: &Path,
    system_prompt: &str,
) -> Result<Vec<ChatMessage>> {
    let path = path.to_path_buf();
    let system_prompt = system_prompt.to_string();
    tokio::task::spawn_blocking(move || {
        load_interactive_session_history(&path, &system_prompt)
    })
    .await?
}

pub(crate) async fn save_interactive_session_history_async(
    path: &Path,
    history: &[ChatMessage],
) -> Result<()> {
    let path = path.to_path_buf();
    let history = history.to_vec();
    tokio::task::spawn_blocking(move || {
        save_interactive_session_history(&path, &history)
    })
    .await?
}
