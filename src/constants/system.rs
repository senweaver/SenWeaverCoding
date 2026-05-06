// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// System constants — mirrors claude-code-typescript-src`constants/system.ts`.

pub const MAX_CONCURRENT_SUBAGENTS: u32 = 8;

pub const MAX_AGENT_DEPTH: u32 = 4;

pub const COMPACTION_THRESHOLD: f64 = 0.8;

pub const DEFAULT_THINKING_BUDGET: u32 = 10_000;

pub const SESSION_IDLE_TIMEOUT_MS: u64 = 3_600_000;

pub const MAX_ERROR_LOG_ENTRIES: usize = 100;

pub const MAX_HISTORY_ITEMS: usize = 100;

pub const MAX_PASTED_CONTENT_LENGTH: usize = 1024;

pub const CLEANUP_POLL_INTERVAL_MS: u64 = 60_000;

pub const HEARTBEAT_INTERVAL_MS: u64 = 30_000;

pub const MAX_TIPS_PER_SESSION: u32 = 3;

pub fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

pub fn default_shell() -> &'static str {
    if cfg!(target_os = "windows") {
        "powershell"
    } else {
        "bash"
    }
}
