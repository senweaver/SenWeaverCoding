// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::providers::ChatMessage;

const MAX_SESSION_HISTORY_BYTES: u64 = 256 * 1024 * 1024;

pub fn estimate_history_tokens(history: &[ChatMessage]) -> usize {
    history
        .iter()
        .map(crate::providers::traits::estimate_message_tokens)
        .sum()
}

pub(crate) fn estimate_tokens_filtered(history: &[ChatMessage], is_system: bool) -> usize {
    history
        .iter()
        .filter(|m| (m.role == "system") == is_system)
        .map(crate::providers::traits::estimate_message_tokens)
        .sum()
}

pub(crate) fn build_compaction_transcript(messages: &[ChatMessage], max_chars: usize) -> String {
    let mut transcript = String::new();
    for msg in messages {
        let role = msg.role.to_uppercase();
        let _ = writeln!(transcript, "{role}: {}", msg.content.trim());
    }

    match crate::util::truncate_head_tail(&transcript, max_chars, 20) {
        Some(clipped) => clipped,
        None => transcript,
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

fn unique_backup_path(path: &Path, ts: &str) -> PathBuf {
    let first = path.with_extension(format!("corrupt.{ts}.json"));
    if !first.exists() {
        return first;
    }
    for n in 1..10_000 {
        let candidate = path.with_extension(format!("corrupt.{ts}.{n}.json"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path.with_extension(format!("corrupt.{ts}.{}.json", std::process::id()))
}

fn backup_and_fresh_session(
    path: &Path,
    system_prompt: &str,
    reason: &str,
    detail: &str,
) -> Vec<ChatMessage> {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let backup = unique_backup_path(path, &ts);
    match std::fs::rename(path, &backup) {
        Ok(()) => tracing::error!(
            file = %path.display(),
            backup = %backup.display(),
            reason,
            detail,
            "interactive session history could not be loaded; backed it up and starting a fresh session"
        ),
        Err(rename_err) => tracing::error!(
            file = %path.display(),
            reason,
            detail,
            rename_error = %rename_err,
            "interactive session history could not be loaded and could not be backed up; starting a fresh session"
        ),
    }
    vec![ChatMessage::system(system_prompt)]
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
            return Ok(backup_and_fresh_session(
                path,
                system_prompt,
                "file too large",
                &format!(
                    "{} bytes exceeds limit {}",
                    meta.len(),
                    MAX_SESSION_HISTORY_BYTES
                ),
            ));
        }
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            return Ok(backup_and_fresh_session(
                path,
                system_prompt,
                "read failed",
                &err.to_string(),
            ));
        }
    };
    let mut state: InteractiveSessionState = match serde_json::from_str(&raw) {
        Ok(state) => state,
        Err(err) => {
            return Ok(backup_and_fresh_session(
                path,
                system_prompt,
                "corrupt json",
                &err.to_string(),
            ));
        }
    };
    if state.history.is_empty() {
        state.history.push(ChatMessage::system(system_prompt));
    } else if state.history.first().map(|msg| msg.role.as_str()) != Some("system") {
        state.history.insert(0, ChatMessage::system(system_prompt));
    }
    for msg in state.history.iter_mut() {
        msg.strip_ephemeral_context();
    }

    Ok(state.history)
}

pub(crate) fn save_interactive_session_history(path: &Path, history: &[ChatMessage]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let payload = serde_json::to_string(&InteractiveSessionState::from_history(history))?;
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
