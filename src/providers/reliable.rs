// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::Provider;
use super::traits::{
    ChatMessage, ChatRequest, ChatResponse, RetryClass, RetryNotice, StreamChunk, StreamError,
    StreamEvent, StreamOptions, StreamResult,
};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

tokio::task_local! {
    static STREAM_CANCEL_TOKEN: Option<CancellationToken>;
}

pub fn scope_stream_cancel_token_sync<F, R>(token: Option<CancellationToken>, f: F) -> R
where
    F: FnOnce() -> R,
{
    STREAM_CANCEL_TOKEN.sync_scope(token, f)
}

pub async fn scope_stream_cancel_token<F: Future>(
    token: Option<CancellationToken>,
    fut: F,
) -> F::Output {
    STREAM_CANCEL_TOKEN.scope(token, fut).await
}

fn current_stream_cancel_token() -> Option<CancellationToken> {
    STREAM_CANCEL_TOKEN
        .try_with(|cell| cell.clone())
        .ok()
        .flatten()
        .or_else(super::current_session_cancel_token)
}

pub fn is_non_retryable(err: &anyhow::Error) -> bool {

    if is_context_window_exceeded(err) {
        return false;
    }

    if is_tool_schema_error(err) {
        return false;
    }

    use crate::error::{ErrorCategory, ErrorClassification};
    match err.category() {
        ErrorCategory::Permission | ErrorCategory::Validation | ErrorCategory::NotFound => {
            return true;
        }
        ErrorCategory::Network | ErrorCategory::Timeout | ErrorCategory::RateLimit => {

        }
        _ => {}
    }

    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>() {
        if let Some(status) = reqwest_err.status() {
            let code = status.as_u16();
            return status.is_client_error() && code != 429 && code != 408;
        }
    }

    let msg = err.to_string();
    if let Some(code) = crate::error::extract_http_status_code(&msg) {
        if (400..500).contains(&code) {
            return code != 429 && code != 408;
        }
    }

    let msg_lower = msg.to_lowercase();
    let auth_failure_hints = [
        "invalid api key",
        "incorrect api key",
        "missing api key",
        "api key not set",
        "authentication failed",
        "auth failed",
        "unauthorized",
        "forbidden",
        "permission denied",
        "access denied",
        "invalid token",
    ];

    if auth_failure_hints
        .iter()
        .any(|hint| msg_lower.contains(hint))
    {
        return true;
    }

    msg_lower.contains("model")
        && (msg_lower.contains("not found")
            || msg_lower.contains("unknown")
            || msg_lower.contains("unsupported")
            || msg_lower.contains("does not exist")
            || msg_lower.contains("invalid"))
}

pub fn is_tool_schema_error(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    let hints = [
        "tool call validation failed",
        "was not in request",
        "not found in tool list",
        "invalid_tool_call",
    ];
    hints.iter().any(|hint| lower.contains(hint))
}

pub(crate) fn is_context_window_exceeded(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    let hints = [
        "exceeds the context window",
        "exceeds the available context size",
        "context window of this model",
        "maximum context length",
        "context length exceeded",
        "too many tokens",
        "token limit exceeded",
        "prompt is too long",
        "input is too long",
        "prompt exceeds max length",
    ];

    hints.iter().any(|hint| lower.contains(hint))
}

fn contains_official_rate_limit_code(lower: &str) -> bool {
    [
        "rate_limit_error",
        "rate_limit_exceeded",
        "too_many_requests",
    ]
    .iter()
    .any(|code| lower.contains(code))
}

fn provider_http_has_official_rate_limit_code(body: &str) -> bool {
    if let Some(code) = super::extract_provider_error_code(body) {
        if crate::providers::traits::is_official_rate_limit_code(&code) {
            return true;
        }
    }
    contains_official_rate_limit_code(&body.to_lowercase())
}

pub(crate) fn is_rate_limited(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(provider_err) = cause.downcast_ref::<crate::providers::ProviderError>() {
            if let Some(status) = provider_err.http_status() {
                if status == 429 {
                    return true;
                }
                if let Some(body) = provider_err.http_body() {
                    if provider_http_has_official_rate_limit_code(body) {
                        return true;
                    }
                }
                return false;
            }
        }
        if let Some(stream_err) = cause.downcast_ref::<StreamError>() {
            if stream_err.is_official_rate_limit() {
                return true;
            }
            if let Some(status) = stream_err.http_status() {
                return status == 429;
            }
        }
        if let Some(reqwest_err) = cause.downcast_ref::<reqwest::Error>() {
            if let Some(status) = reqwest_err.status() {
                return status.as_u16() == 429;
            }
        }
    }
    let msg = err.to_string();
    if crate::error::extract_http_status_code(&msg) == Some(429) {
        return true;
    }
    contains_official_rate_limit_code(&msg.to_lowercase())
}

pub(crate) fn is_transport_level_error(err: &anyhow::Error) -> bool {
    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>() {
        if reqwest_err.is_connect() || reqwest_err.is_timeout() {
            return true;
        }
        if reqwest_err.is_request() && reqwest_err.status().is_none() {
            return true;
        }
        return false;
    }
    let lower = err.to_string().to_lowercase();
    [
        "error sending request for url",
        "error decoding response body",
        "connection refused",
        "connection reset",
        "connection aborted",
        "connection closed",
        "broken pipe",
        "dns error",
        "tls handshake",
        "operation timed out",
        "request timeout",
        "unexpected end of file",
        "stream ended before",
        "io error",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

fn contains_official_overload_code(lower: &str) -> bool {
    [
        "overloaded_error",
        "engine_overloaded",
        "server_overloaded",
        "service_overloaded",
    ]
    .iter()
    .any(|code| lower.contains(code))
}

fn contains_overload_hint(lower: &str) -> bool {
    [
        "engine overload",
        "engine is currently overloaded",
        "engine is overloaded",
        "service overloaded",
        "currently overloaded",
        "temporarily overloaded",
        "system overloaded",
        "upstream overload",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

fn overload_capable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 529)
}

pub(crate) fn is_engine_overloaded(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(provider_err) = cause.downcast_ref::<crate::providers::ProviderError>() {
            if let Some(status) = provider_err.http_status() {
                if status == 529 {
                    return true;
                }
                let body = provider_err.http_body().unwrap_or("").to_lowercase();
                return overload_capable_status(status)
                    && (contains_official_overload_code(&body) || contains_overload_hint(&body));
            }
        }
        if let Some(stream_err) = cause.downcast_ref::<StreamError>() {
            if stream_err.is_official_overload() {
                return true;
            }
            if let Some(status) = stream_err.http_status() {
                if status == 529 {
                    return true;
                }
                let message = stream_err.to_string().to_lowercase();
                return overload_capable_status(status)
                    && (contains_official_overload_code(&message)
                        || contains_overload_hint(&message));
            }
        }
        if let Some(reqwest_err) = cause.downcast_ref::<reqwest::Error>() {
            if let Some(status) = reqwest_err.status() {
                return status.as_u16() == 529;
            }
        }
    }
    let lower = err.to_string().to_lowercase();
    if contains_official_overload_code(&lower) {
        return true;
    }
    match crate::error::extract_http_status_code(&lower) {
        Some(529) => true,
        Some(status) => overload_capable_status(status) && contains_overload_hint(&lower),
        None => false,
    }
}

fn is_account_rate_limited(err: &anyhow::Error) -> bool {
    if !is_rate_limited(err) {
        return false;
    }
    if is_engine_overloaded(err) {
        return false;
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureClass {
    EngineOverloaded,
    AccountRateLimited,
    NonRetryable,
    Transient,
}

impl FailureClass {
    fn from_error(err: &anyhow::Error) -> Self {
        if is_non_retryable(err) || is_non_retryable_rate_limit(err) {
            return FailureClass::NonRetryable;
        }
        if is_engine_overloaded(err) {
            return FailureClass::EngineOverloaded;
        }
        if is_account_rate_limited(err) {
            return FailureClass::AccountRateLimited;
        }
        FailureClass::Transient
    }

    fn as_failure_reason(self, rate_limited: bool) -> &'static str {
        match self {
            FailureClass::EngineOverloaded => "engine_overloaded",
            FailureClass::AccountRateLimited => "rate_limited",
            FailureClass::NonRetryable => {
                if rate_limited {
                    "rate_limited_non_retryable"
                } else {
                    "non_retryable"
                }
            }
            FailureClass::Transient => "retryable",
        }
    }
}

pub fn is_non_retryable_rate_limit(err: &anyhow::Error) -> bool {
    if !is_rate_limited(err) {
        return false;
    }

    let msg = err.to_string();
    let lower = msg.to_lowercase();

    let business_hints = [
        "plan does not include",
        "doesn't include",
        "not include",
        "insufficient balance",
        "insufficient_balance",
        "insufficient quota",
        "insufficient_quota",
        "quota exhausted",
        "out of credits",
        "no available package",
        "package not active",
        "purchase package",
        "model not available for your plan",
    ];

    if business_hints.iter().any(|hint| lower.contains(hint)) {
        return true;
    }

    for token in lower.split(|c: char| !c.is_ascii_digit()) {
        if let Ok(code) = token.parse::<u16>() {
            if matches!(code, 1113 | 1311) {
                return true;
            }
        }
    }

    false
}

pub(crate) const RETRY_AFTER_CAP_MS: u64 = 300_000;

fn retry_after_ms_from_text(msg: &str) -> Option<u64> {
    if let Some(ms) = super::retry_after_ms_from_prefixed_body(msg) {
        return Some(ms);
    }

    let lower = msg.to_lowercase();
    let status = crate::error::extract_http_status_code(&lower);
    let retry_after_eligible = matches!(status, Some(429) | Some(503) | Some(529))
        || contains_official_rate_limit_code(&lower)
        || contains_official_overload_code(&lower);
    if !retry_after_eligible {
        return None;
    }

    for prefix in &[
        "retry-after:",
        "retry_after:",
        "retry-after ",
        "retry_after ",
    ] {
        if let Some(pos) = lower.find(prefix) {
            let after = msg[pos + prefix.len()..].trim_start();
            let line_end = after.find('\n').unwrap_or(after.len());
            if let Some(ms) = super::parse_retry_after_value_ms(after[..line_end].trim()) {
                return Some(ms);
            }
        }
    }

    if let Some(pos) = lower.find("retrydelay") {
        let after = &lower[pos + "retrydelay".len()..];
        if let Some(start) = after.find(|c: char| c.is_ascii_digit()) {
            if start <= 6 {
                let rest = &after[start..];
                let num_str: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                let tail = &rest[num_str.len()..];
                if let Ok(value) = num_str.parse::<f64>() {
                    if value.is_finite() && value >= 0.0 {
                        let ms = if tail.starts_with("ms") {
                            value
                        } else {
                            value * 1000.0
                        };
                        return Some(ms.round() as u64);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn parse_retry_after_ms(err: &anyhow::Error) -> Option<u64> {
    for cause in err.chain() {
        if let Some(stream_err) = cause.downcast_ref::<StreamError>() {
            if let Some(ms) = stream_err.retry_after_ms() {
                return Some(ms);
            }
        }
        if let Some(provider_err) = cause.downcast_ref::<crate::providers::ProviderError>() {
            if let Some(body) = provider_err.http_body() {
                if let Some(ms) = retry_after_ms_from_text(body) {
                    return Some(ms);
                }
            }
        }
    }
    retry_after_ms_from_text(&err.to_string())
}

fn pseudo_jitter_seed(attempt: u32) -> f64 {
    let deterministic =
        (attempt.wrapping_mul(2_654_435_761) as f64 / u32::MAX as f64).clamp(0.0, 1.0);
    let random = rand::random::<f64>();
    ((deterministic + random) * 0.5).clamp(0.0, 1.0)
}

fn class_backoff_ms(attempt: u32, class: FailureClass) -> Option<u64> {
    let schedule: &[u64] = match class {
        FailureClass::EngineOverloaded => &[
            1_000, 2_000, 4_000, 8_000, 15_000, 30_000, 30_000, 30_000, 30_000, 30_000,
        ],
        FailureClass::AccountRateLimited => {
            &[2_000, 4_000, 8_000, 15_000, 30_000, 60_000, 60_000, 60_000, 60_000, 60_000]
        }
        _ => return None,
    };
    let idx = (attempt as usize).min(schedule.len() - 1);
    let base = schedule[idx] as f64;
    let jitter_ratio = 1.0 + (pseudo_jitter_seed(attempt) - 0.5) * 0.4;
    let millis = (base * jitter_ratio).max(0.0) as u64;
    Some(millis.min(60_000))
}

fn summarize_dominant_class(failures: &[String]) -> Option<FailureClass> {
    let mut overload = 0usize;
    let mut rate_limit = 0usize;
    let mut transient = 0usize;
    let mut non_retryable = 0usize;
    for f in failures {
        if f.contains("engine_overloaded") {
            overload += 1;
        } else if f.contains("rate_limited_non_retryable") {
            non_retryable += 1;
        } else if f.contains("rate_limited") {
            rate_limit += 1;
        } else if f.contains("non_retryable") {
            non_retryable += 1;
        } else if f.contains("retryable") {
            transient += 1;
        }
    }
    let total = overload + rate_limit + transient + non_retryable;
    if total == 0 {
        return None;
    }
    let max = overload.max(rate_limit).max(transient).max(non_retryable);
    if max == overload {
        Some(FailureClass::EngineOverloaded)
    } else if max == rate_limit {
        Some(FailureClass::AccountRateLimited)
    } else if max == non_retryable {
        Some(FailureClass::NonRetryable)
    } else {
        Some(FailureClass::Transient)
    }
}

fn final_failure_message(
    failures: &[String],
    suffix: &str,
    attempts: u32,
    engine_overload_attempts: u32,
    rate_limit_attempts: u32,
) -> String {
    let dominant = summarize_dominant_class(failures);
    let header = match dominant {
        Some(FailureClass::EngineOverloaded) => format!(
            "All providers/models failed after {attempts} attempts due to upstream engine overload \
             (HTTP 429 engine_overloaded_error or equivalent). This is a temporary server-side issue, \
             not a client-side rate limit. Try again in 1-2 minutes, or switch to a fallback model \
             via reliability.fallback_providers / model_fallbacks. ({engine_overload_attempts} \
             engine-overload retries observed.){suffix}"
        ),
        Some(FailureClass::AccountRateLimited) => format!(
            "All providers/models failed after {attempts} attempts due to account-level rate limit \
             (HTTP 429 rate_limit_exceeded / TPM / RPM). Check your account quota or wait for the \
             window to reset. ({rate_limit_attempts} rate-limit retries observed.){suffix}"
        ),
        Some(FailureClass::NonRetryable) => format!(
            "All providers/models failed after {attempts} attempts; the dominant failure was \
             non-retryable (invalid key, missing model, validation, etc.). Verify provider \
             credentials and model availability.{suffix}"
        ),
        _ => format!(
            "All providers/models failed after {attempts} attempts. Inspect the per-attempt log \
             below to diagnose.{suffix}"
        ),
    };
    format!("{header}\nAttempts:\n{}", failures.join("\n"))
}

fn compact_error_detail(err: &anyhow::Error) -> String {
    super::sanitize_api_error(&err.to_string())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_for_context(messages: &mut Vec<ChatMessage>) -> usize {

    let non_system: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role != "system")
        .map(|(i, _)| i)
        .collect();

    if non_system.len() <= 1 {
        return 0;
    }

    let drop_count = (non_system.len() / 4).max(2).min(non_system.len() - 1);
    let mut end = drop_count;
    while end < non_system.len()
        && messages
            .get(non_system[end])
            .is_some_and(|m| m.role == "tool")
    {
        end += 1;
    }
    let end = end.min(non_system.len() - 1);
    let indices_to_remove: Vec<usize> = non_system[..end].to_vec();
    let removed = indices_to_remove.len();

    for &idx in indices_to_remove.iter().rev() {
        messages.remove(idx);
    }

    removed
}

fn push_failure(
    failures: &mut Vec<String>,
    provider_name: &str,
    model: &str,
    attempt: u32,
    max_attempts: u32,
    reason: &str,
    error_detail: &str,
) {
    failures.push(format!(
        "provider={provider_name} model={model} attempt {attempt}/{max_attempts}: {reason}; error={error_detail}"
    ));
}

pub const TRANSIENT_RETRY_FLOOR: u32 = 12;

pub const TRANSPORT_RETRY_CAP: u32 = 12;

const STREAM_BACKOFF_CEILING_MS: u64 = 10_000;

const STREAM_CONTEXT_TRUNCATION_MAX: u32 = 4;

pub struct ReliableProvider {
    providers: Vec<(String, Arc<dyn Provider>)>,
    max_retries: u32,
    base_backoff_ms: u64,

    model_fallbacks: HashMap<String, Vec<String>>,

    counter: std::sync::Arc<crate::providers::core::retry::ReliabilityCounter>,

    engine_overload_max_retries: u32,

    account_rate_limit_max_retries: u32,

    transient_max_retries: u32,
}

impl ReliableProvider {
    async fn scope_stream_retry<F>(
        session: Option<crate::session::SessionContext>,
        mode: Option<crate::agent::coding_mode::CodingMode>,
        fut: F,
    ) where
        F: std::future::Future<Output = ()>,
    {
        let mode_scoped = async move {
            match mode {
                Some(m) => crate::agent::coding_mode::scope_coding_mode(m, fut).await,
                None => fut.await,
            }
        };
        match session {
            Some(ctx) => crate::session::scope_session_context(ctx, mode_scoped).await,
            None => mode_scoped.await,
        }
    }

    pub fn new(
        providers: Vec<(String, Box<dyn Provider>)>,
        max_retries: u32,
        base_backoff_ms: u64,
    ) -> Self {
        let providers = providers
            .into_iter()
            .map(|(name, boxed)| (name, Arc::<dyn Provider>::from(boxed)))
            .collect();
        let max_retries = max_retries.max(1);
        Self {
            providers,
            max_retries,
            base_backoff_ms: base_backoff_ms.max(50),
            model_fallbacks: HashMap::new(),
            counter: std::sync::Arc::new(crate::providers::core::retry::ReliabilityCounter::new()),
            engine_overload_max_retries: 10,
            account_rate_limit_max_retries: 5,
            transient_max_retries: max_retries.max(TRANSIENT_RETRY_FLOOR),
        }
    }

    pub fn with_retry_caps(
        mut self,
        engine_overload_max_retries: u32,
        account_rate_limit_max_retries: u32,
    ) -> Self {
        self.engine_overload_max_retries = engine_overload_max_retries.max(1);
        self.account_rate_limit_max_retries = account_rate_limit_max_retries.max(1);
        self
    }

    pub fn with_transient_max_retries(mut self, transient_max_retries: u32) -> Self {
        self.transient_max_retries = transient_max_retries.max(TRANSIENT_RETRY_FLOOR);
        self
    }

    pub fn counter(&self) -> std::sync::Arc<crate::providers::core::retry::ReliabilityCounter> {
        std::sync::Arc::clone(&self.counter)
    }

    pub fn spawn_health_tick(&self, provider: String, model: String) {
        let counter = std::sync::Arc::clone(&self.counter);
        crate::runtime::spawn_supervised("providers.reliable.health_tick", async move {
            let window = crate::agent::health_signal::HEALTH_WINDOW;
            loop {
                tokio::time::sleep(window).await;
                let snap = counter.snapshot();
                counter.reset();
                if let Some(svc) = crate::services::try_get_services() {
                    let signal = crate::agent::health_signal::HealthSignal {
                        provider: provider.clone(),
                        model: model.clone(),
                        window_secs: window.as_secs(),
                        success_rate: snap.success_rate,

                        p95_latency_ms: snap.avg_latency_ms,
                        retries_per_req: if snap.successes + snap.failures > 0 {
                            snap.retries as f64 / (snap.successes + snap.failures) as f64
                        } else {
                            0.0
                        },
                        cost_per_1k_tok: 0.0,
                    };
                    svc.health_broadcaster.publish(signal);
                }
            }
        });
    }

    pub fn with_model_fallbacks(mut self, fallbacks: HashMap<String, Vec<String>>) -> Self {
        self.model_fallbacks = fallbacks;
        self
    }

    fn model_chain<'a>(&'a self, model: &'a str) -> Vec<&'a str> {
        let mut chain = vec![model];
        if let Some(fallbacks) = self.model_fallbacks.get(model) {
            chain.extend(fallbacks.iter().map(|s| s.as_str()));
        }
        chain
    }

    fn compute_backoff_for_class(
        &self,
        attempt: u32,
        err: &anyhow::Error,
        class: FailureClass,
    ) -> u64 {
        let honor_retry_after = match class {
            FailureClass::AccountRateLimited | FailureClass::EngineOverloaded => true,
            FailureClass::Transient => matches!(
                crate::error::extract_http_status_code(&err.to_string()),
                Some(503)
            ),
            FailureClass::NonRetryable => false,
        };
        if honor_retry_after {
            if let Some(retry_after) = parse_retry_after_ms(err) {
                return retry_after.min(RETRY_AFTER_CAP_MS);
            }
        }
        if let Some(ms) = class_backoff_ms(attempt, class) {
            return ms;
        }
        crate::providers::core::retry::exp_backoff(attempt, self.base_backoff_ms, 30_000, 0.25)
            .as_millis() as u64
    }

    fn effective_class_cap(&self, class: FailureClass) -> u32 {
        match class {
            FailureClass::EngineOverloaded => self.engine_overload_max_retries,
            FailureClass::AccountRateLimited => self.account_rate_limit_max_retries,
            FailureClass::Transient => self.transient_max_retries,
            FailureClass::NonRetryable => self.max_retries,
        }
    }

    fn attempt_idempotency_key(
        provider: &str,
        model: &str,
        messages: &serde_json::Value,
        tools: &serde_json::Value,
    ) -> String {
        let key =
            crate::providers::core::idempotency::fingerprint_json(provider, model, messages, tools)
                .into_inner();
        tracing::debug!(
            idempotency_key = %key,
            provider,
            model,
            "dispatching provider request"
        );
        key
    }

    async fn with_idempotency<F>(
        provider: &str,
        model: &str,
        messages: &serde_json::Value,
        tools: &serde_json::Value,
        fut: F,
    ) -> F::Output
    where
        F: std::future::Future,
    {
        let key = Self::attempt_idempotency_key(provider, model, messages, tools);
        crate::providers::core::idempotency::scope_idempotency_key(key, fut).await
    }

    fn classify_retry(err: &anyhow::Error) -> crate::providers::core::retry::RetryClass {
        if is_non_retryable(err) {
            return crate::providers::core::retry::RetryClass::Permanent;
        }
        if is_engine_overloaded(err) {
            return crate::providers::core::retry::RetryClass::Transient;
        }
        if is_rate_limited(err) {
            return crate::providers::core::retry::RetryClass::RateLimited;
        }
        crate::providers::core::retry::RetryClass::Transient
    }

    fn outer_retry_cap(&self) -> u32 {
        self.max_retries
            .max(self.engine_overload_max_retries)
            .max(self.account_rate_limit_max_retries)
            .max(self.transient_max_retries)
    }

    fn handle_attempt_failure(
        &self,
        failures: &mut Vec<String>,
        provider_name: &str,
        current_model: &str,
        attempt: u32,
        state: &mut RetryState,
        err: &anyhow::Error,
    ) -> FailureAction {
        let class = FailureClass::from_error(err);
        let rate_limited = is_rate_limited(err);
        let error_detail = compact_error_detail(err);
        let reason = class.as_failure_reason(rate_limited);
        let cap_for_class = self.effective_class_cap(class);

        push_failure(
            failures,
            provider_name,
            current_model,
            attempt + 1,
            cap_for_class + 1,
            reason,
            &error_detail,
        );

        match class {
            FailureClass::NonRetryable => {
                tracing::warn!(
                    provider = provider_name,
                    model = current_model,
                    error = %error_detail,
                    "Non-retryable error, moving on"
                );
                return FailureAction::NonRetryable;
            }
            FailureClass::EngineOverloaded => state.engine_overload_attempts += 1,
            FailureClass::AccountRateLimited => state.rate_limit_attempts += 1,
            FailureClass::Transient => state.transient_attempts += 1,
        }

        let is_transport = matches!(class, FailureClass::Transient) && is_transport_level_error(err);
        if is_transport {
            state.transport_attempts += 1;
            if state.transport_attempts >= TRANSPORT_RETRY_CAP {
                tracing::warn!(
                    provider = provider_name,
                    model = current_model,
                    transport_attempts = state.transport_attempts,
                    cap = TRANSPORT_RETRY_CAP,
                    error = %error_detail,
                    "Transport-level failure cap reached; short-circuiting current provider/model"
                );
                return FailureAction::ExhaustedClass;
            }
        }

        let class_attempts = match class {
            FailureClass::EngineOverloaded => state.engine_overload_attempts,
            FailureClass::AccountRateLimited => state.rate_limit_attempts,
            FailureClass::Transient => state.transient_attempts,
            FailureClass::NonRetryable => attempt + 1,
        };

        if class_attempts >= cap_for_class {
            tracing::warn!(
                provider = provider_name,
                model = current_model,
                class = ?class,
                attempts = class_attempts,
                cap = cap_for_class,
                "Provider exhausted retry budget for failure class, moving on"
            );
            return FailureAction::ExhaustedClass;
        }

        let class_attempt_index = class_attempts.saturating_sub(1);
        let wait = self
            .compute_backoff_for_class(class_attempt_index, err, class)
            .min(if matches!(class, FailureClass::Transient) {
                STREAM_BACKOFF_CEILING_MS
            } else {
                RETRY_AFTER_CAP_MS
            });
        let retry_class = Self::classify_retry(err);
        tracing::warn!(
            provider = provider_name,
            model = current_model,
            attempt = attempt + 1,
            backoff_ms = wait,
            reason,
            retry_class = ?retry_class,
            failure_class = ?class,
            error = %error_detail,
            "Provider call failed, retrying"
        );
        FailureAction::Retry { sleep_ms: wait }
    }

    fn stream_chunks_with_failover<F>(
        &self,
        model: &str,
        options: StreamOptions,
        make_stream: F,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>>
    where
        F: Fn(Arc<dyn Provider>, String) -> stream::BoxStream<'static, StreamResult<StreamChunk>>
            + Send
            + 'static,
    {
        let mut candidates: Vec<(String, Arc<dyn Provider>)> = Vec::new();
        if options.enabled {
            for (provider_name, provider) in &self.providers {
                if provider.supports_streaming() {
                    candidates.push((provider_name.clone(), Arc::clone(provider)));
                }
            }
        }

        if candidates.is_empty() {
            return stream::once(async move {
                Err(StreamError::Provider(
                    "No provider supports streaming".to_string(),
                ))
            })
            .boxed();
        }

        let model_list: Vec<String> = {
            let chain: Vec<String> = self
                .model_chain(model)
                .iter()
                .map(|m| (*m).to_string())
                .collect();
            if chain.is_empty() {
                vec![model.to_string()]
            } else {
                chain
            }
        };

        let mut combos: Vec<(String, Arc<dyn Provider>, String)> = Vec::new();
        for current_model in &model_list {
            for (label, prov) in &candidates {
                combos.push((label.clone(), Arc::clone(prov), current_model.clone()));
            }
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let cancel_token = current_stream_cancel_token();

        let _bg = crate::runtime::spawn_supervised(
            "providers.reliable.stream_chunks_failover",
            async move {
                let mut last_err: Option<StreamError> = None;

                for (provider_label, provider_arc, current_model) in combos {
                    let mut stream = make_stream(provider_arc, current_model.clone());
                    let mut made_progress = false;
                    let mut failed_before_progress = false;

                    loop {
                        let item = if let Some(token) = cancel_token.as_ref() {
                            tokio::select! {
                                biased;
                                () = token.cancelled() => {
                                    let _ = tx
                                        .send(Err(StreamError::Provider(
                                            "stream cancelled by user".to_string(),
                                        )))
                                        .await;
                                    return;
                                }
                                item = stream.next() => item,
                            }
                        } else {
                            stream.next().await
                        };
                        let Some(item) = item else { break };
                        match item {
                            Ok(chunk) => {
                                made_progress = true;
                                if tx.send(Ok(chunk)).await.is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                if made_progress {
                                    tracing::warn!(
                                        provider = %provider_label,
                                        model = %current_model,
                                        "Streaming error after first chunk: {e}"
                                    );
                                    let _ = tx.send(Err(e)).await;
                                    return;
                                }
                                tracing::warn!(
                                    provider = %provider_label,
                                    model = %current_model,
                                    error = %e,
                                    "streaming failed before first chunk; failing over to next streaming candidate"
                                );
                                last_err = Some(e);
                                failed_before_progress = true;
                                break;
                            }
                        }
                    }

                    if made_progress {
                        return;
                    }
                    if !failed_before_progress {
                        tracing::warn!(
                            provider = %provider_label,
                            model = %current_model,
                            "stream produced no chunks; failing over to next streaming candidate"
                        );
                        last_err = Some(StreamError::Provider(
                            "upstream stream produced no chunks".to_string(),
                        ));
                    }
                }

                let _ = tx
                    .send(Err(last_err.unwrap_or_else(|| {
                        StreamError::Provider(
                            "all streaming providers/models failed".to_string(),
                        )
                    })))
                    .await;
            },
        );

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        })
        .boxed()
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct RetryState {
    engine_overload_attempts: u32,
    rate_limit_attempts: u32,
    transient_attempts: u32,
    transport_attempts: u32,
}

#[derive(Debug, Clone, Copy)]
enum FailureAction {
    NonRetryable,
    ExhaustedClass,
    Retry { sleep_ms: u64 },
}

#[async_trait]
impl Provider for ReliableProvider {
    async fn warmup(&self) -> anyhow::Result<()> {
        for (name, provider) in &self.providers {
            tracing::info!(provider = name, "Warming up provider connection pool");
            if provider.warmup().await.is_err() {
                tracing::warn!(provider = name, "Warmup failed (non-fatal)");
            }
        }
        Ok(())
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let models = self.model_chain(model);
        let mut failures = Vec::new();
        let request_value = serde_json::json!({
            "system": system_prompt,
            "message": message,
        });

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                state = RetryState::default();
                for attempt in 0..=outer_cap {
                    match Self::with_idempotency(
                        provider_name,
                        current_model,
                        &request_value,
                        &serde_json::Value::Null,
                        provider.chat_with_system(system_prompt, message, current_model, temperature),
                    )
                    .await
                    {
                        Ok(resp) => {
                            if attempt > 0
                                || *current_model != model
                                || self.providers.first().map(|(n, _)| n.as_str())
                                    != Some(provider_name)
                            {
                                tracing::info!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt,
                                    original_model = model,
                                    "Provider recovered (failover/retry)"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            if is_context_window_exceeded(&e) {
                                let error_detail = compact_error_detail(&e);
                                push_failure(
                                    &mut failures,
                                    provider_name,
                                    current_model,
                                    attempt + 1,
                                    outer_cap + 1,
                                    "non_retryable",
                                    &error_detail,
                                );
                                anyhow::bail!(
                                    "Request exceeds model context window. Attempts:\n{}",
                                    failures.join("\n")
                                );
                            }

                            match self.handle_attempt_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt,
                                &mut state,
                                &e,
                            ) {
                                FailureAction::NonRetryable | FailureAction::ExhaustedClass => {
                                    break;
                                }
                                FailureAction::Retry { sleep_ms } => {
                                    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                                }
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name,
                    model = *current_model,
                    "Exhausted retries, trying next provider/model"
                );
            }

            if *current_model != model {
                tracing::warn!(
                    original_model = model,
                    fallback_model = *current_model,
                    "Model fallback exhausted all providers, trying next fallback model"
                );
            }
        }

        let total_attempts = failures.len() as u32;
        anyhow::bail!(
            "{}",
            final_failure_message(
                &failures,
                "",
                total_attempts,
                state.engine_overload_attempts,
                state.rate_limit_attempts,
            )
        )
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let models = self.model_chain(model);
        let mut failures = Vec::new();
        let mut effective_messages = messages.to_vec();
        let mut context_truncated = false;

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                state = RetryState::default();
                for attempt in 0..=outer_cap {
                    let messages_value = serde_json::to_value(&effective_messages)
                        .unwrap_or(serde_json::Value::Null);
                    match Self::with_idempotency(
                        provider_name,
                        current_model,
                        &messages_value,
                        &serde_json::Value::Null,
                        provider.chat_with_history(&effective_messages, current_model, temperature),
                    )
                    .await
                    {
                        Ok(resp) => {
                            if attempt > 0
                                || *current_model != model
                                || context_truncated
                                || self.providers.first().map(|(n, _)| n.as_str())
                                    != Some(provider_name)
                            {
                                tracing::info!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt,
                                    original_model = model,
                                    context_truncated,
                                    "Provider recovered (failover/retry)"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            if is_context_window_exceeded(&e) {
                                if effective_messages.len() > 2 {
                                    let dropped = truncate_for_context(&mut effective_messages);
                                    if dropped > 0 {
                                        context_truncated = true;
                                        tracing::warn!(
                                            provider = provider_name,
                                            model = *current_model,
                                            dropped,
                                            remaining = effective_messages.len(),
                                            "Context window exceeded; truncated history and retrying"
                                        );
                                        continue;
                                    }
                                }

                                let error_detail = compact_error_detail(&e);
                                push_failure(
                                    &mut failures,
                                    provider_name,
                                    current_model,
                                    attempt + 1,
                                    outer_cap + 1,
                                    "non_retryable",
                                    &error_detail,
                                );
                                anyhow::bail!(
                                    "Request exceeds model context window and cannot be reduced further. \
                                     Try using a model with a larger context window, reducing the number \
                                     of tools/skills, or enabling compact_context in config. Attempts:\n{}",
                                    failures.join("\n")
                                );
                            }

                            match self.handle_attempt_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt,
                                &mut state,
                                &e,
                            ) {
                                FailureAction::NonRetryable | FailureAction::ExhaustedClass => {
                                    break;
                                }
                                FailureAction::Retry { sleep_ms } => {
                                    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                                }
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name,
                    model = *current_model,
                    "Exhausted retries, trying next provider/model"
                );
            }
        }

        let total_attempts = failures.len() as u32;
        anyhow::bail!(
            "{}",
            final_failure_message(
                &failures,
                "",
                total_attempts,
                state.engine_overload_attempts,
                state.rate_limit_attempts,
            )
        )
    }

    fn supports_native_tools(&self) -> bool {
        self.providers
            .first()
            .map(|(_, p)| p.supports_native_tools())
            .unwrap_or(false)
    }

    fn supports_vision(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, provider)| provider.supports_vision())
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let models = self.model_chain(model);
        let mut failures = Vec::new();
        let mut effective_messages = messages.to_vec();
        let mut context_truncated = false;
        let tools_value = serde_json::Value::Array(tools.to_vec());

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                state = RetryState::default();
                for attempt in 0..=outer_cap {
                    let messages_value = serde_json::to_value(&effective_messages)
                        .unwrap_or(serde_json::Value::Null);
                    match Self::with_idempotency(
                        provider_name,
                        current_model,
                        &messages_value,
                        &tools_value,
                        provider.chat_with_tools(&effective_messages, tools, current_model, temperature),
                    )
                    .await
                    {
                        Ok(resp) => {
                            if attempt > 0
                                || *current_model != model
                                || context_truncated
                                || self.providers.first().map(|(n, _)| n.as_str())
                                    != Some(provider_name)
                            {
                                tracing::info!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt,
                                    original_model = model,
                                    context_truncated,
                                    "Provider recovered (failover/retry)"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            if is_context_window_exceeded(&e) {
                                if effective_messages.len() > 2 {
                                    let dropped = truncate_for_context(&mut effective_messages);
                                    if dropped > 0 {
                                        context_truncated = true;
                                        tracing::warn!(
                                            provider = provider_name,
                                            model = *current_model,
                                            dropped,
                                            remaining = effective_messages.len(),
                                            "Context window exceeded; truncated history and retrying"
                                        );
                                        continue;
                                    }
                                }

                                let error_detail = compact_error_detail(&e);
                                push_failure(
                                    &mut failures,
                                    provider_name,
                                    current_model,
                                    attempt + 1,
                                    outer_cap + 1,
                                    "non_retryable",
                                    &error_detail,
                                );
                                anyhow::bail!(
                                    "Request exceeds model context window and cannot be reduced further. \
                                     Try using a model with a larger context window, reducing the number \
                                     of tools/skills, or enabling compact_context in config. Attempts:\n{}",
                                    failures.join("\n")
                                );
                            }

                            match self.handle_attempt_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt,
                                &mut state,
                                &e,
                            ) {
                                FailureAction::NonRetryable | FailureAction::ExhaustedClass => {
                                    break;
                                }
                                FailureAction::Retry { sleep_ms } => {
                                    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                                }
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name,
                    model = *current_model,
                    "Exhausted retries, trying next provider/model"
                );
            }
        }

        let total_attempts = failures.len() as u32;
        anyhow::bail!(
            "{}",
            final_failure_message(
                &failures,
                "",
                total_attempts,
                state.engine_overload_attempts,
                state.rate_limit_attempts,
            )
        )
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let models = self.model_chain(model);
        let mut failures = Vec::new();
        let mut effective_messages = request.messages.to_vec();
        let mut context_truncated = false;
        let tools_value = request
            .tools
            .and_then(|t| serde_json::to_value(t).ok())
            .unwrap_or(serde_json::Value::Null);

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                state = RetryState::default();
                for attempt in 0..=outer_cap {
                    let req = ChatRequest {
                        messages: &effective_messages,
                        tools: request.tools,
                    };
                    let messages_value = serde_json::to_value(&effective_messages)
                        .unwrap_or(serde_json::Value::Null);
                    match Self::with_idempotency(
                        provider_name,
                        current_model,
                        &messages_value,
                        &tools_value,
                        provider.chat(req, current_model, temperature),
                    )
                    .await
                    {
                        Ok(resp) => {
                            if attempt > 0
                                || *current_model != model
                                || context_truncated
                                || self.providers.first().map(|(n, _)| n.as_str())
                                    != Some(provider_name)
                            {
                                tracing::info!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt,
                                    original_model = model,
                                    context_truncated,
                                    "Provider recovered (failover/retry)"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            if is_context_window_exceeded(&e) {
                                if effective_messages.len() > 2 {
                                    let dropped = truncate_for_context(&mut effective_messages);
                                    if dropped > 0 {
                                        context_truncated = true;
                                        tracing::warn!(
                                            provider = provider_name,
                                            model = *current_model,
                                            dropped,
                                            remaining = effective_messages.len(),
                                            "Context window exceeded; truncated history and retrying"
                                        );
                                        continue;
                                    }
                                }

                                let error_detail = compact_error_detail(&e);
                                push_failure(
                                    &mut failures,
                                    provider_name,
                                    current_model,
                                    attempt + 1,
                                    outer_cap + 1,
                                    "non_retryable",
                                    &error_detail,
                                );
                                anyhow::bail!(
                                    "Request exceeds model context window and cannot be reduced further. \
                                     Try using a model with a larger context window, reducing the number \
                                     of tools/skills, or enabling compact_context in config. Attempts:\n{}",
                                    failures.join("\n")
                                );
                            }

                            match self.handle_attempt_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt,
                                &mut state,
                                &e,
                            ) {
                                FailureAction::NonRetryable | FailureAction::ExhaustedClass => {
                                    break;
                                }
                                FailureAction::Retry { sleep_ms } => {
                                    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                                }
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name,
                    model = *current_model,
                    "Exhausted retries, trying next provider/model"
                );
            }

            if *current_model != model {
                tracing::warn!(
                    original_model = model,
                    fallback_model = *current_model,
                    "Model fallback exhausted all providers, trying next fallback model"
                );
            }
        }

        let total_attempts = failures.len() as u32;
        anyhow::bail!(
            "{}",
            final_failure_message(
                &failures,
                "",
                total_attempts,
                state.engine_overload_attempts,
                state.rate_limit_attempts,
            )
        )
    }

    fn supports_streaming(&self) -> bool {
        self.providers.iter().any(|(_, p)| p.supports_streaming())
    }

    fn supports_streaming_tool_events(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, p)| p.supports_streaming_tool_events())
    }

    fn stream_chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
        let needs_tool_events = request.tools.is_some_and(|tools| !tools.is_empty());

        let mut candidates: Vec<(String, Arc<dyn Provider>)> = Vec::new();
        if options.enabled {
            for (provider_name, provider) in &self.providers {
                if !provider.supports_streaming() {
                    continue;
                }
                if needs_tool_events && !provider.supports_streaming_tool_events() {
                    continue;
                }
                candidates.push((provider_name.clone(), Arc::clone(provider)));
            }
        }

        if !candidates.is_empty() {
            let model_list: Vec<String> = {
                let chain: Vec<String> =
                    self.model_chain(model).iter().map(|m| m.to_string()).collect();
                if chain.is_empty() {
                    vec![model.to_string()]
                } else {
                    chain
                }
            };

            let mut combos: Vec<(String, Arc<dyn Provider>, String)> = Vec::new();
            for current_model in &model_list {
                for (label, prov) in &candidates {
                    combos.push((label.clone(), Arc::clone(prov), current_model.clone()));
                }
            }

            let mut messages_owned: Vec<ChatMessage> = request.messages.to_vec();
            let tools_owned: Option<Arc<Vec<crate::tools::ToolSpec>>> =
                request.tools.map(|t| Arc::new(t.to_vec()));

            let engine_cap = self.engine_overload_max_retries;
            let account_cap = self.account_rate_limit_max_retries;
            let transient_cap = self
                .transient_max_retries
                .max(self.max_retries)
                .max(TRANSIENT_RETRY_FLOOR);
            let base_backoff = self.base_backoff_ms;
            let cancel_token = current_stream_cancel_token();
            let scoped_session = crate::session::current_session_context();
            let scoped_mode = crate::agent::coding_mode::scoped_coding_mode();
            let session_label = scoped_session
                .as_ref()
                .map(|ctx| ctx.session_id.clone())
                .unwrap_or_default();

            let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

            let _bg = crate::runtime::spawn_supervised(
                "providers.reliable.stream_chat_retry",
                Self::scope_stream_retry(scoped_session, scoped_mode, async move {
                    let total_combos = combos.len();
                    let mut last_failure: Option<String> = None;
                    let mut context_truncation_passes: u32 = 0;
                    let tools_value = tools_owned
                        .as_deref()
                        .and_then(|t| serde_json::to_value(t).ok())
                        .unwrap_or(serde_json::Value::Null);

                    for (combo_idx, (provider_label, provider_arc, current_model)) in
                        combos.into_iter().enumerate()
                    {
                        let is_last_combo = combo_idx + 1 >= total_combos;

                        let mut engine_overload_attempts: u32 = 0;
                        let mut rate_limit_attempts: u32 = 0;
                        let mut transient_attempts: u32 = 0;
                        let mut transport_attempts: u32 = 0;

                        let (summary, switch_class): (String, RetryClass) = 'retry: loop {
                            if let Some(token) = cancel_token.as_ref() {
                                if token.is_cancelled() {
                                    let _ = tx
                                        .send(Err(StreamError::Provider(
                                            "stream cancelled by user".to_string(),
                                        )))
                                        .await;
                                    return;
                                }
                            }

                            let req = ChatRequest {
                                messages: messages_owned.as_slice(),
                                tools: tools_owned.as_deref().map(|v| v.as_slice()),
                            };
                            let messages_value = serde_json::to_value(&messages_owned)
                                .unwrap_or(serde_json::Value::Null);
                            let idem_key = Self::attempt_idempotency_key(
                                &provider_label,
                                &current_model,
                                &messages_value,
                                &tools_value,
                            );
                            let mut stream =
                                crate::providers::core::idempotency::scope_idempotency_key_sync(
                                    idem_key,
                                    || {
                                        provider_arc.stream_chat(
                                            req,
                                            &current_model,
                                            temperature,
                                            options,
                                        )
                                    },
                                );

                            let mut made_progress = false;
                            let mut last_err: Option<StreamError> = None;
                            let mut stream_finished_cleanly = false;

                            loop {
                                let next = if let Some(token) = cancel_token.as_ref() {
                                    tokio::select! {
                                        biased;
                                        () = token.cancelled() => {
                                            let _ = tx
                                                .send(Err(StreamError::Provider(
                                                    "stream cancelled by user".to_string(),
                                                )))
                                                .await;
                                            return;
                                        }
                                        item = stream.next() => item,
                                    }
                                } else {
                                    stream.next().await
                                };

                                let Some(event) = next else {
                                    stream_finished_cleanly = true;
                                    break;
                                };

                                match event {
                                    Ok(ev) => {
                                        if matches!(
                                            ev,
                                            StreamEvent::TextDelta(_)
                                                | StreamEvent::ToolCall(_)
                                                | StreamEvent::PreExecutedToolCall { .. }
                                                | StreamEvent::PreExecutedToolResult { .. }
                                        ) {
                                            made_progress = true;
                                        }
                                        let is_final = matches!(ev, StreamEvent::Final);
                                        if is_final && !made_progress {
                                            break 'retry (
                                                "Upstream emitted Final with no preceding output (empty stream)".to_string(),
                                                RetryClass::Transient,
                                            );
                                        }
                                        if tx.send(Ok(ev)).await.is_err() {
                                            return;
                                        }
                                        if is_final {
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        last_err = Some(e);
                                        break;
                                    }
                                }
                            }

                            let Some(err) = last_err else {
                                if stream_finished_cleanly && !made_progress {
                                    break 'retry (
                                        "Upstream stream produced no events".to_string(),
                                        RetryClass::Transient,
                                    );
                                }
                                let _ = tx.send(Ok(StreamEvent::Final)).await;
                                return;
                            };

                            let err_string = err.to_string();

                            if made_progress {
                                break 'retry (
                                    format!(
                                        "Streaming interrupted after partial output on \
                                         {provider_label}/{current_model}; not retrying the same \
                                         provider to avoid resending duplicate output. Failing over \
                                         to the next streaming candidate if available. Last error: \
                                         {err_string}"
                                    ),
                                    RetryClass::Transient,
                                );
                            }

                            let anyhow_err = anyhow::Error::new(err);

                            if is_context_window_exceeded(&anyhow_err) {
                                if context_truncation_passes < STREAM_CONTEXT_TRUNCATION_MAX {
                                    let removed = truncate_for_context(&mut messages_owned);
                                    if removed > 0 {
                                        context_truncation_passes += 1;
                                        tracing::warn!(
                                            target: "providers.reliable.retry",
                                            session_id = %session_label,
                                            provider = %provider_label,
                                            model = %current_model,
                                            removed,
                                            pass = context_truncation_passes,
                                            "stream request exceeded the context window; dropped oldest messages and retrying"
                                        );
                                        let notice = RetryNotice {
                                            attempt: context_truncation_passes,
                                            max_attempts: STREAM_CONTEXT_TRUNCATION_MAX,
                                            wait_ms: 0,
                                            failure_class: RetryClass::Transient,
                                            provider: provider_label.clone(),
                                            model: current_model.clone(),
                                            last_error_summary: format!(
                                                "context window exceeded; dropped {removed} oldest messages and retrying"
                                            ),
                                        };
                                        if tx.send(Ok(StreamEvent::Retry(notice))).await.is_err() {
                                            return;
                                        }
                                        continue 'retry;
                                    }
                                }
                                break 'retry (
                                    format!(
                                        "Request exceeds the model context window and dropping older \
                                         messages could not shrink it enough (passes={context_truncation_passes}). \
                                         Reduce the conversation size or switch to a larger-context \
                                         model. Last error: {err_string}"
                                    ),
                                    RetryClass::Transient,
                                );
                            }

                            let class = FailureClass::from_error(&anyhow_err);

                            let (class_attempts, cap_for_class, retry_class) = match class {
                                FailureClass::EngineOverloaded => {
                                    engine_overload_attempts += 1;
                                    (
                                        engine_overload_attempts,
                                        engine_cap,
                                        RetryClass::EngineOverloaded,
                                    )
                                }
                                FailureClass::AccountRateLimited => {
                                    rate_limit_attempts += 1;
                                    (
                                        rate_limit_attempts,
                                        account_cap,
                                        RetryClass::AccountRateLimited,
                                    )
                                }
                                FailureClass::Transient => {
                                    transient_attempts += 1;
                                    if is_transport_level_error(&anyhow_err) {
                                        transport_attempts += 1;
                                        if transport_attempts >= TRANSPORT_RETRY_CAP {
                                            let summary = format!(
                                                "Transport-level streaming failure on {provider_label}/{current_model} \
                                                 reached cap={TRANSPORT_RETRY_CAP} (e.g. connect/send/decoding errors). \
                                                 Stopping retries against this provider/model so an outer fallback can \
                                                 pick a different one. Last error: {err_string}"
                                            );
                                            break 'retry (summary, RetryClass::Transient);
                                        }
                                    }
                                    (transient_attempts, transient_cap, RetryClass::Transient)
                                }
                                FailureClass::NonRetryable => {
                                    break 'retry (err_string, RetryClass::Transient);
                                }
                            };

                            if class_attempts > cap_for_class {
                                let summary = match class {
                                    FailureClass::EngineOverloaded => format!(
                                        "Upstream engine overloaded (HTTP 429 engine_overloaded_error or equivalent) after {class_attempts} streaming attempts; cap={cap_for_class}. \
                                         This is a temporary server-side issue, not a client-side rate limit. Try again in 1-2 minutes, or switch to a fallback model \
                                         via reliability.fallback_providers / model_fallbacks. Last error: {err_string}"
                                    ),
                                    FailureClass::AccountRateLimited => format!(
                                        "Account-level rate limit (HTTP 429 rate_limit_exceeded / TPM / RPM) after {class_attempts} streaming attempts; cap={cap_for_class}. \
                                         Check your account quota or wait for the window to reset. Last error: {err_string}"
                                    ),
                                    FailureClass::Transient => format!(
                                        "Transient streaming error persisted after {class_attempts} attempts; cap={cap_for_class}. Last error: {err_string}"
                                    ),
                                    FailureClass::NonRetryable => err_string.clone(),
                                };
                                break 'retry (summary, retry_class);
                            }

                            let class_attempt_index = class_attempts.saturating_sub(1);
                            let wait_ms_raw = if let Some(ms) = parse_retry_after_ms(&anyhow_err) {
                                ms.min(RETRY_AFTER_CAP_MS)
                            } else if let Some(ms) =
                                class_backoff_ms(class_attempt_index, class)
                            {
                                ms
                            } else {
                                crate::providers::core::retry::exp_backoff(
                                    class_attempt_index,
                                    base_backoff,
                                    30_000,
                                    0.25,
                                )
                                .as_millis() as u64
                            };
                            let wait_ms = if matches!(class, FailureClass::Transient) {
                                wait_ms_raw.min(STREAM_BACKOFF_CEILING_MS)
                            } else {
                                wait_ms_raw
                            };

                            let last_error_summary = compact_error_detail(&anyhow_err);

                            tracing::warn!(
                                target: "providers.reliable.retry",
                                session_id = %session_label,
                                provider = %provider_label,
                                model = %current_model,
                                attempt = class_attempts,
                                cap = cap_for_class,
                                wait_ms,
                                failure_class = ?class,
                                error = %last_error_summary,
                                "stream provider returned retryable failure; emitting RetryNotice and scheduling re-attempt"
                            );

                            let notice = RetryNotice {
                                attempt: class_attempts,
                                max_attempts: cap_for_class,
                                wait_ms,
                                failure_class: retry_class,
                                provider: provider_label.clone(),
                                model: current_model.clone(),
                                last_error_summary,
                            };

                            if tx.send(Ok(StreamEvent::Retry(notice))).await.is_err() {
                                return;
                            }

                            const RETRY_WAIT_HEARTBEAT_MS: u64 = 60_000;
                            let mut remaining_ms = wait_ms;
                            loop {
                                let slice_ms = remaining_ms.min(RETRY_WAIT_HEARTBEAT_MS);
                                let slice = Duration::from_millis(slice_ms);
                                if let Some(token) = cancel_token.as_ref() {
                                    tokio::select! {
                                        biased;
                                        () = token.cancelled() => {
                                            let _ = tx
                                                .send(Err(StreamError::Provider(
                                                    "stream cancelled by user during retry wait".to_string(),
                                                )))
                                                .await;
                                            return;
                                        }
                                        () = tokio::time::sleep(slice) => {}
                                    }
                                } else {
                                    tokio::time::sleep(slice).await;
                                }
                                remaining_ms = remaining_ms.saturating_sub(slice_ms);
                                if remaining_ms == 0 {
                                    break;
                                }
                                let heartbeat = RetryNotice {
                                    attempt: class_attempts,
                                    max_attempts: cap_for_class,
                                    wait_ms: remaining_ms,
                                    failure_class: retry_class,
                                    provider: provider_label.clone(),
                                    model: current_model.clone(),
                                    last_error_summary: format!(
                                        "waiting out upstream backoff; {}s remaining",
                                        remaining_ms.div_ceil(1000)
                                    ),
                                };
                                if tx.send(Ok(StreamEvent::Retry(heartbeat))).await.is_err() {
                                    return;
                                }
                            }
                        };

                        last_failure = Some(summary.clone());
                        if is_last_combo {
                            let _ = tx.send(Err(StreamError::Provider(summary))).await;
                            return;
                        }
                        tracing::warn!(
                            target: "providers.reliable.retry",
                            session_id = %session_label,
                            provider = %provider_label,
                            model = %current_model,
                            "streaming provider/model exhausted; failing over to next streaming candidate"
                        );
                        let notice = RetryNotice {
                            attempt: 1,
                            max_attempts: 1,
                            wait_ms: 0,
                            failure_class: switch_class,
                            provider: provider_label.clone(),
                            model: current_model.clone(),
                            last_error_summary: summary,
                        };
                        if tx.send(Ok(StreamEvent::Retry(notice))).await.is_err() {
                            return;
                        }
                    }

                    let _ = tx
                        .send(Err(StreamError::Provider(last_failure.unwrap_or_else(
                            || "all streaming providers/models failed".to_string(),
                        ))))
                        .await;
                }),
            );

            return stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            })
            .boxed();
        }

        let message = if needs_tool_events {
            "No provider supports streaming tool events".to_string()
        } else {
            "No provider supports streaming".to_string()
        };
        stream::once(async move { Err(super::traits::StreamError::Provider(message)) }).boxed()
    }

    fn stream_chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let system_owned = system_prompt.map(ToString::to_string);
        let message_owned = message.to_string();

        self.stream_chunks_with_failover(model, options, move |provider, current_model| {
            provider.stream_chat_with_system(
                system_owned.as_deref(),
                &message_owned,
                &current_model,
                temperature,
                options,
            )
        })
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let messages_owned: Arc<Vec<ChatMessage>> = Arc::new(messages.to_vec());

        self.stream_chunks_with_failover(model, options, move |provider, current_model| {
            provider.stream_chat_with_history(
                messages_owned.as_slice(),
                &current_model,
                temperature,
                options,
            )
        })
    }
}
