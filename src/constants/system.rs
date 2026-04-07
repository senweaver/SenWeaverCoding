// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// System constants — mirrors claude-code-typescript-src`constants/system.ts`.

/// Maximum concurrent sub-agents.
pub const MAX_CONCURRENT_SUBAGENTS: u32 = 8;

/// Maximum agent nesting depth.
pub const MAX_AGENT_DEPTH: u32 = 4;

/// Default compaction threshold (fraction of context window).
pub const COMPACTION_THRESHOLD: f64 = 0.8;

/// Default thinking budget tokens.
pub const DEFAULT_THINKING_BUDGET: u32 = 10_000;

/// Session idle timeout (milliseconds) — 1 hour.
pub const SESSION_IDLE_TIMEOUT_MS: u64 = 3_600_000;

/// Maximum in-memory error log entries.
pub const MAX_ERROR_LOG_ENTRIES: usize = 100;

/// Maximum number of conversation history items.
pub const MAX_HISTORY_ITEMS: usize = 100;

/// Maximum pasted content length before external storage (chars).
pub const MAX_PASTED_CONTENT_LENGTH: usize = 1024;

/// Cleanup registry poll interval (milliseconds).
pub const CLEANUP_POLL_INTERVAL_MS: u64 = 60_000;

/// Heartbeat interval (milliseconds).
pub const HEARTBEAT_INTERVAL_MS: u64 = 30_000;

/// Maximum number of tips shown per session.
pub const MAX_TIPS_PER_SESSION: u32 = 3;

/// Get the current platform name.
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

/// Get the default shell for the current platform.
pub fn default_shell() -> &'static str {
    if cfg!(target_os = "windows") {
        "powershell"
    } else {
        "bash"
    }
}
