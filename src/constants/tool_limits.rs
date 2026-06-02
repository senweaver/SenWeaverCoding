// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const MAX_TOOL_OUTPUT_CHARS: usize = 30_000;

pub const TOOL_TIMEOUT_MS: u64 = 120_000;

pub const SHELL_TIMEOUT_MS: u64 = 300_000;

pub const FILE_READ_MAX_LINES: usize = 2000;

pub const FILE_WRITE_MAX_BYTES: usize = 1_048_576;

pub const WEB_FETCH_MAX_BYTES: usize = 5 * 1024 * 1024;

pub const WEB_FETCH_TIMEOUT_MS: u64 = 30_000;

pub const MAX_SEARCH_RESULTS: usize = 50;

pub const MAX_GREP_OUTPUT_LINES: usize = 500;

pub const MAX_BATCH_FILES: usize = 100;

pub const MCP_TOOL_TIMEOUT_MS: u64 = 60_000;

pub const AGENT_TOOL_TIMEOUT_MS: u64 = 600_000;

pub const MAX_TOOL_CALLS_PER_TURN: u32 = 256;

pub const TRUNCATION_MESSAGE: &str = "\n... [output truncated]";
