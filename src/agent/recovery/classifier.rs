// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
    let status = crate::error::extract_http_status_code(m);

    if status == Some(429)
        || m.contains("rate_limit_error")
        || m.contains("rate_limit_exceeded")
        || m.contains("too_many_requests")
        || m.contains("rate limit")
        || m.contains("too many requests")
        || m.contains("quota exceeded")
    {
        return ErrorClass::RateLimited;
    }

    if status == Some(401)
        || status == Some(403)
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
        || matches!(status, Some(502) | Some(503) | Some(504))
        || m.contains("gateway")
    {
        return ErrorClass::Transient;
    }
    ErrorClass::Unknown
}
