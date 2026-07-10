// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::observability::tui_metrics;

#[derive(Debug)]
pub struct ChatMessageReconciler {

    pub last_session_version: u64,

    pub last_turn_count: usize,

    pub last_turn_seq: u64,

    pub last_mirror_len: usize,
}

impl Default for ChatMessageReconciler {
    fn default() -> Self {
        Self {
            last_session_version: 0,
            last_turn_count: 0,
            last_turn_seq: 0,
            last_mirror_len: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {

    NoSession,

    Noop,

    Incremental,

    Backfilled,
}

impl ChatMessageReconciler {

    pub fn reset(&mut self) {
        self.last_session_version = 0;
        self.last_turn_count = 0;
        self.last_turn_seq = 0;
        self.last_mirror_len = 0;
    }

    pub fn reconcile(
        &mut self,
        chat_messages: &mut Vec<crate::tui::ChatMessage>,
        actor_slot: &std::sync::Arc<
            once_cell::sync::OnceCell<std::sync::Arc<crate::session::SessionActor>>,
        >,
    ) -> ReconcileOutcome {
        let Some(actor) = actor_slot.get() else {
            return ReconcileOutcome::NoSession;
        };
        let current_version = actor.version();
        tui_metrics::set_tui_chat_messages_version(current_version);

        if current_version == self.last_session_version {
            tui_metrics::incr_tui_chat_reconcile_noop();
            return ReconcileOutcome::Noop;
        }
        self.last_session_version = current_version;

        let new_turn_count = actor.turn_count();
        let mirror_len_now = chat_messages.len();

        if new_turn_count <= self.last_turn_count {
            self.last_turn_count = new_turn_count;
            self.last_turn_seq = actor.last_turn_seq();
            self.last_mirror_len = mirror_len_now;
            tui_metrics::incr_tui_chat_reconcile_incremental();
            return ReconcileOutcome::Incremental;
        }

        let new_turns = actor.turns_since(self.last_turn_seq);

        let ts = chrono::Utc::now().format("%H:%M:%S").to_string();
        let mut appended = 0usize;
        let mut max_seq = self.last_turn_seq;
        for turn in &new_turns {
            max_seq = max_seq.max(turn.seq);

            if chat_messages
                .iter()
                .rev()
                .take(6)
                .any(|m| m.role == "user" && m.content == turn.input)
            {
                continue;
            }
            chat_messages.push(crate::tui::ChatMessage::from_parts(
                "system",
                format!("↯ peer turn (#{}): {}", turn.seq, turn.input),
                ts.clone(),
            ));
            appended += 1;
        }

        self.last_turn_count = new_turn_count;
        self.last_turn_seq = max_seq;
        self.last_mirror_len = mirror_len_now + appended;

        if appended == 0 {
            tui_metrics::incr_tui_chat_reconcile_incremental();
            ReconcileOutcome::Incremental
        } else {
            tui_metrics::incr_tui_chat_reconcile_full();
            ReconcileOutcome::Backfilled
        }
    }
}
