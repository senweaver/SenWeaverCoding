// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn within_replay_window(timestamp_unix_secs: &str, window_secs: u64) -> bool {
    let Ok(ts) = timestamp_unix_secs.parse::<u64>() else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let delta = ts.abs_diff(now);
    delta <= window_secs
}
