// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use once_cell::sync::Lazy;
use regex::Regex;

static NO_MODEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)no[_\s\-]?model[_\s\-]?configured|please\s+add\s+at\s+least\s+one\s+model|未添加模型")
        .expect("no-model-configured error regex must compile")
});

pub fn is_no_model_error(message: &str) -> bool {
    NO_MODEL_RE.is_match(message)
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

pub fn classify_turn_error_code(message: &str) -> &'static str {
    if is_no_model_error(message) {
        return "NO_MODEL_CONFIGURED";
    }

    let lower = message.to_ascii_lowercase();
    let status = crate::error::extract_http_status_code(&lower);

    if contains_any(
        &lower,
        &["turn was cancelled", "turn was canceled", "turn cancelled", "turn canceled"],
    ) || contains_any(message, &["已取消", "用户取消"])
    {
        return "CANCELLED";
    }

    if lower.contains("repeated_model_response") {
        return "REPEATED_MODEL_RESPONSE";
    }

    if contains_any(
        &lower,
        &[
            "insufficient balance",
            "insufficient_quota",
            "insufficient quota",
            "exceeded_current_quota",
            "exceeded current quota",
            "quota exceeded",
            "exceeded your current quota",
            "billing",
            "recharge",
            "out of credits",
            "no credit",
            "payment required",
            "account is suspended",
            "account suspended",
        ],
    ) || contains_any(message, &["余额不足", "额度不足", "配额不足", "欠费", "请充值"])
    {
        return "INSUFFICIENT_BALANCE";
    }

    if contains_any(
        &lower,
        &[
            "data_inspection_failed",
            "content_filter",
            "content filter",
            "contentfilter",
            "inappropriate content",
            "content management policy",
            "prohibited_content",
            "prohibited content",
            "sensitive content",
            "content policy violation",
            "violates our usage policy",
            "flagged as potentially violating",
        ],
    ) || contains_any(message, &["敏感内容", "内容安全", "涉敏"])
    {
        return "CONTENT_FILTERED";
    }

    if contains_any(
        &lower,
        &[
            "invalid api key",
            "invalid_api_key",
            "incorrect api key",
            "authentication",
            "unauthorized",
            "permission denied",
            "forbidden",
        ],
    ) || matches!(status, Some(401) | Some(403))
    {
        return "AUTH_ERROR";
    }

    if contains_any(
        &lower,
        &[
            "model not found",
            "model_not_found",
            "no such model",
            "model does not exist",
            "does not exist or you do not have access",
            "unsupported model",
            "the model `",
        ],
    ) {
        return "MODEL_UNAVAILABLE";
    }

    if contains_any(
        &lower,
        &[
            "overloaded_error",
            "engine_overloaded",
            "server_overloaded",
            "service_overloaded",
            "server is busy",
            "service unavailable",
        ],
    ) || matches!(status, Some(503) | Some(529))
    {
        return "ENGINE_OVERLOADED";
    }

    if contains_any(
        &lower,
        &[
            "rate_limit_error",
            "rate_limit_exceeded",
            "too_many_requests",
        ],
    ) || status == Some(429)
    {
        return "RATE_LIMITED";
    }

    if contains_any(
        &lower,
        &["timed out", "timeout", "deadline exceeded", "timeout exceeded"],
    ) {
        return "CONNECTION_TIMEOUT";
    }

    if contains_any(
        &lower,
        &[
            "connection refused",
            "failed to connect",
            "connect error",
            "dns error",
            "could not resolve",
            "network is unreachable",
            "tls handshake",
        ],
    ) {
        return "CONNECTION_FAILED";
    }

    if contains_any(&lower, &["bad gateway", "internal server error"])
        || matches!(status, Some(500) | Some(502) | Some(504))
    {
        return "GATEWAY_ERROR";
    }

    "AGENT_TURN_FAILED"
}

pub fn user_facing_error_json(message: &str, code: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "error",
        "error": message,
        "message": message,
        "code": code,
        "detail": message,
    })
}
