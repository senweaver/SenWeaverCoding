// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Classify tool / provider errors so the loop can decide whether to
//! retry transient failures, escalate rate-limit back-off, or surface
//! permanent problems to the user.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {

    Transient,

    RateLimited,

    Permanent,

    Unknown,
}

pub fn classify_tool_error(err: &anyhow::Error) -> ErrorClass {
    let msg = format!("{err:#}").to_lowercase();
    classify_message(&msg)
}

pub fn classify_str(msg: &str) -> ErrorClass {
    classify_message(&msg.to_lowercase())
}

fn classify_message(m: &str) -> ErrorClass {

    if m.contains("429")
        || m.contains("rate limit")
        || m.contains("rate_limit")
        || m.contains("too many requests")
        || m.contains("quota exceeded")
    {
        return ErrorClass::RateLimited;
    }

    if m.contains("401")
        || m.contains("403")
        || m.contains("unauthorized")
        || m.contains("forbidden")
        || m.contains("invalid api key")
        || m.contains("authentication failed")
        || m.contains("permission denied")
    {
        return ErrorClass::Permanent;
    }

    if m.contains("timed out")
        || m.contains("timeout")
        || m.contains("connection reset")
        || m.contains("connection refused")
        || m.contains("broken pipe")
        || m.contains("temporarily unavailable")
        || m.contains("503")
        || m.contains("502")
        || m.contains("504")
        || m.contains("gateway")
    {
        return ErrorClass::Transient;
    }
    ErrorClass::Unknown
}
