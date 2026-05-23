// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::channels::traits;

pub(crate) fn conversation_memory_key(msg: &traits::ChannelMessage) -> String {
    match &msg.thread_ts {
        Some(tid) => format!("{}_{}_{}_{}", msg.channel, tid, msg.sender, msg.id),
        None => format!("{}_{}_{}", msg.channel, msg.sender, msg.id),
    }
}

pub(crate) fn conversation_history_key(msg: &traits::ChannelMessage) -> String {
    match &msg.thread_ts {
        Some(tid) => format!(
            "{}_{}_{}_{}",
            msg.channel, msg.reply_target, tid, msg.sender
        ),
        None => format!("{}_{}_{}", msg.channel, msg.reply_target, msg.sender),
    }
}

pub(crate) fn followup_thread_id(msg: &traits::ChannelMessage) -> Option<String> {
    msg.thread_ts.clone().or_else(|| Some(msg.id.clone()))
}

pub(crate) fn interruption_scope_key(msg: &traits::ChannelMessage) -> String {
    match &msg.interruption_scope_id {
        Some(scope) => format!(
            "{}_{}_{}_{}",
            msg.channel, msg.reply_target, msg.sender, scope
        ),
        None => format!("{}_{}_{}", msg.channel, msg.reply_target, msg.sender),
    }
}

pub(crate) fn is_stop_command(content: &str) -> bool {
    let trimmed = content.trim();
    if !trimmed.starts_with('/') {
        return false;
    }
    let cmd = trimmed.split_whitespace().next().unwrap_or("");
    let base = cmd.split('@').next().unwrap_or(cmd);
    base.eq_ignore_ascii_case("/stop")
}
