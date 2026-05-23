// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

use crate::providers::traits::ChatMessage;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnFinalizeOutcome {

    pub was_trimmed: bool,

    pub was_compressed: bool,

    pub messages_before: usize,

    pub messages_after: usize,

    pub session_persisted: bool,

    pub session_bytes: usize,
}

impl TurnFinalizeOutcome {

    pub fn compression_ratio(&self) -> Option<f64> {
        if self.messages_before == 0 {
            return None;
        }
        Some(self.messages_after as f64 / self.messages_before as f64)
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.was_trimmed {
            parts.push(format!(
                "trimmed {}→{}",
                self.messages_before, self.messages_after
            ));
        }
        if self.was_compressed {
            parts.push("compressed".into());
        }
        if self.session_persisted {
            parts.push(format!("persisted ({}b)", self.session_bytes));
        }
        if parts.is_empty() {
            "noop".into()
        } else {
            parts.join(", ")
        }
    }
}

pub trait FinalizePolicy {

    fn trim_ceiling(&self, history_len: usize) -> Option<usize>;

    fn should_compress(&self, history_len: usize) -> bool;
}

#[derive(Debug, Clone, Copy)]
pub struct BudgetFinalizePolicy {
    pub max_messages: usize,
    pub compress_threshold: usize,
}

impl BudgetFinalizePolicy {

    pub const DEFAULT: Self = Self {
        max_messages: 100,
        compress_threshold: 200,
    };
}

impl FinalizePolicy for BudgetFinalizePolicy {
    fn trim_ceiling(&self, history_len: usize) -> Option<usize> {
        if history_len > self.max_messages {
            Some(self.max_messages)
        } else {
            None
        }
    }

    fn should_compress(&self, history_len: usize) -> bool {
        history_len > self.compress_threshold
    }
}

pub async fn finalize_turn(
    history: &mut Vec<ChatMessage>,
    policy: &dyn FinalizePolicy,
    session_state_file: Option<&Path>,
) -> TurnFinalizeOutcome {
    let mut outcome = TurnFinalizeOutcome {
        messages_before: history.len(),
        ..Default::default()
    };

    if let Some(ceiling) = policy.trim_ceiling(history.len()) {
        let drop_count = history.len().saturating_sub(ceiling);
        if drop_count > 0 {

            let start = usize::from(history.first().map(|m| m.role == "system").unwrap_or(false));
            let end = (start + drop_count).min(history.len());
            if end > start {
                history.drain(start..end);
                outcome.was_trimmed = true;
            }
        }
    }

    if policy.should_compress(history.len()) {
        outcome.was_compressed = true;
        let keep_recent = 20.min(history.len());
        let system_head: Vec<ChatMessage> = history
            .iter()
            .take_while(|m| m.role == "system")
            .cloned()
            .collect();
        let tail_start = history.len().saturating_sub(keep_recent);
        let tail: Vec<ChatMessage> = history[tail_start..].to_vec();
        let compressed_count = history.len().saturating_sub(system_head.len() + tail.len());
        if compressed_count > 0 {
            let mut rebuilt = system_head;
            rebuilt.push(ChatMessage::system(format!(
                "[compressed {compressed_count} earlier messages]"
            )));
            rebuilt.extend(tail);
            *history = rebuilt;
            outcome.was_trimmed = true;
        }
    }

    outcome.messages_after = history.len();

    if let Some(path) = session_state_file {
        let payload = serde_json::to_string(history).unwrap_or_default();
        outcome.session_bytes = payload.len();
        match tokio::fs::write(path, payload.as_bytes()).await {
            Ok(()) => outcome.session_persisted = true,
            Err(err) => {
                tracing::debug!(
                    target: "agent.turn_finalize",
                    error = %err,
                    path = %path.display(),
                    "failed to persist session state"
                );
                outcome.session_persisted = false;
            }
        }
    }

    outcome
}
