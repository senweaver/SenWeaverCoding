// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::observability::tui_metrics;

#[derive(Debug)]
pub struct ChatMessageReconciler {

    pub last_session_version: u64,

    pub last_turn_count: usize,

    pub last_mirror_len: usize,
}

impl Default for ChatMessageReconciler {
    fn default() -> Self {
        Self {
            last_session_version: 0,
            last_turn_count: 0,
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
        let snapshot = actor.snapshot();
        tui_metrics::set_tui_chat_messages_version(snapshot.version);

        if snapshot.version == self.last_session_version {
            tui_metrics::incr_tui_chat_reconcile_noop();
            return ReconcileOutcome::Noop;
        }

        let new_turn_count = snapshot.turns.len();
        let mirror_len_now = chat_messages.len();
        self.last_session_version = snapshot.version;

        if new_turn_count <= self.last_turn_count {
            self.last_turn_count = new_turn_count;
            self.last_mirror_len = mirror_len_now;
            tui_metrics::incr_tui_chat_reconcile_incremental();
            return ReconcileOutcome::Incremental;
        }

        let ts = chrono::Utc::now().format("%H:%M:%S").to_string();
        let mut appended = 0usize;
        for turn in snapshot.turns.iter().skip(self.last_turn_count) {

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
