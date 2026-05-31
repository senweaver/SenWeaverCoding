// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::channels::traits;

pub(crate) fn conversation_history_key(msg: &traits::ChannelMessage) -> String {
    match &msg.thread_ts {
        Some(tid) => format!(
            "{}_{}_{}_{}",
            msg.channel, msg.reply_target, tid, msg.sender
        ),
        None => format!("{}_{}_{}", msg.channel, msg.reply_target, msg.sender),
    }
}
