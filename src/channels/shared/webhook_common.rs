// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Common webhook plumbing: replay protection and inbound buffering.
//!
//! first helper is the signature-window check used by
//! Slack-style webhooks that carry a `X-<channel>-Timestamp` header
//! and reject requests more than ±5 minutes old (replay defense).

use std::time::{SystemTime, UNIX_EPOCH};

pub fn within_replay_window(timestamp_unix_secs: &str, window_secs: u64) -> bool {
    let Ok(ts) = timestamp_unix_secs.parse::<u64>() else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let delta = if ts > now { ts - now } else { now - ts };
    delta <= window_secs
}
