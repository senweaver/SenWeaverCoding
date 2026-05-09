// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const ERROR_PREFIXES: &[&str] = &[
    "Error:",
    "error:",
    "Error executing ",
    "Unknown tool: ",
    "[Tool error]",
    "[Refused]",
    "[Cancelled by user]",
    "Tool failed:",
    "Blocked by guardrails:",
    "RBAC denied:",
    "Cancelled by hook:",
];

const ERROR_SUBSTRINGS: &[&str] = &[
    "error sending request for url",
    "failed to send request",
    "connection refused",
    "connection reset",
    "operation timed out",
    "DNS resolution",
    "dns error",
    "tls handshake",
];

pub fn output_indicates_error(output: &str) -> bool {
    let trimmed = output.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    if ERROR_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
        return true;
    }
    let lowered_head: String = trimmed.chars().take(512).collect();
    let lowered = lowered_head.to_ascii_lowercase();
    ERROR_SUBSTRINGS.iter().any(|s| lowered.contains(s))
}
