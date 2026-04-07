// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Tool limits — mirrors claude-code-typescript-src`constants/toolLimits.ts`.

/// Maximum characters in tool output before truncation.
pub const MAX_TOOL_OUTPUT_CHARS: usize = 30_000;

/// Default tool execution timeout (milliseconds).
pub const TOOL_TIMEOUT_MS: u64 = 120_000; // 2 minutes

/// Shell command timeout (milliseconds).
pub const SHELL_TIMEOUT_MS: u64 = 300_000; // 5 minutes

/// File read maximum lines.
pub const FILE_READ_MAX_LINES: usize = 2000;

/// File write maximum size (bytes).
pub const FILE_WRITE_MAX_BYTES: usize = 1_048_576; // 1 MB

/// Web fetch maximum response size (bytes).
pub const WEB_FETCH_MAX_BYTES: usize = 5 * 1024 * 1024; // 5 MB

/// Web fetch timeout (milliseconds).
pub const WEB_FETCH_TIMEOUT_MS: u64 = 30_000;

/// Maximum number of search results.
pub const MAX_SEARCH_RESULTS: usize = 50;

/// Maximum grep output lines.
pub const MAX_GREP_OUTPUT_LINES: usize = 500;

/// Maximum number of files in a batch operation.
pub const MAX_BATCH_FILES: usize = 100;

/// MCP tool execution timeout (milliseconds).
pub const MCP_TOOL_TIMEOUT_MS: u64 = 60_000;

/// Agent tool (sub-agent) timeout (milliseconds) — 10 minutes.
pub const AGENT_TOOL_TIMEOUT_MS: u64 = 600_000;

/// Maximum number of tool calls per turn.
pub const MAX_TOOL_CALLS_PER_TURN: u32 = 32;

/// Truncation message appended when output is cut.
pub const TRUNCATION_MESSAGE: &str = "\n... [output truncated]";
