// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::ConversationMessage;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepairReport {

    pub patches_applied: usize,

    pub frames_touched: usize,
}

pub fn repair_dangling_with_report(
    messages: Vec<ConversationMessage>,
) -> (Vec<ConversationMessage>, RepairReport) {
    let (frames_touched, patches_applied) =
        crate::agent::dangling_tool_repair::count_incomplete_followup_batches(&messages);

    let repaired = crate::agent::dangling_tool_repair::repair_dangling_tool_calls(messages);
    (
        repaired,
        RepairReport {
            patches_applied,
            frames_touched,
        },
    )
}

pub fn repair_dangling(messages: Vec<ConversationMessage>) -> Vec<ConversationMessage> {
    crate::agent::dangling_tool_repair::repair_dangling_tool_calls(messages)
}
