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
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
}

#[derive(Debug, Clone)]
pub struct ProviderFallbackInfo {

    pub requested_provider: String,

    pub requested_model: String,

    pub actual_provider: String,

    pub actual_model: String,
}

tokio::task_local! {
    static PROVIDER_FALLBACK: RefCell<Option<ProviderFallbackInfo>>;
}

pub fn take_last_provider_fallback() -> Option<ProviderFallbackInfo> {
    PROVIDER_FALLBACK
        .try_with(|cell| cell.borrow_mut().take())
        .ok()
        .flatten()
}

pub async fn scope_provider_fallback<F: std::future::Future>(future: F) -> F::Output {
    PROVIDER_FALLBACK.scope(RefCell::new(None), future).await
}

fn record_provider_fallback(
    requested_provider: &str,
    requested_model: &str,
    actual_provider: &str,
    actual_model: &str,
) {
    let _ = PROVIDER_FALLBACK.try_with(|cell| {
        *cell.borrow_mut() = Some(ProviderFallbackInfo {
            requested_provider: requested_provider.to_string(),
            requested_model: requested_model.to_string(),
            actual_provider: actual_provider.to_string(),
            actual_model: actual_model.to_string(),
        });
    });
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
    for word in msg.split(|c: char| !c.is_ascii_digit()) {
        if let Ok(code) = word.parse::<u16>() {
            if (400..500).contains(&code) {
                return code != 429 && code != 408;
            }
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

fn is_rate_limited(err: &anyhow::Error) -> bool {
    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>() {
        if let Some(status) = reqwest_err.status() {
            return status.as_u16() == 429;
        }
    }
    let msg = err.to_string();
    msg.contains("429")
        && (msg.contains("Too Many") || msg.contains("rate") || msg.contains("limit"))
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

fn is_engine_overloaded(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    engine_overload_hints()
        .iter()
        .any(|hint| lower.contains(hint))
}

fn engine_overload_hints() -> &'static [&'static str] {
    &[
        "engine_overloaded",
        "engine overload",
        "engine is currently overloaded",
        "engine is overloaded",
        "overloaded_error",
        "server_overloaded",
        "service overloaded",
        "service_overloaded",
        "currently overloaded",
        "temporarily overloaded",
        "system overloaded",
        "upstream overload",
    ]
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

fn is_non_retryable_rate_limit(err: &anyhow::Error) -> bool {
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

fn parse_retry_after_ms(err: &anyhow::Error) -> Option<u64> {
    let msg = err.to_string();
    let lower = msg.to_lowercase();

    for prefix in &[
        "retry-after:",
        "retry_after:",
        "retry-after ",
        "retry_after ",
    ] {
        if let Some(pos) = lower.find(prefix) {
            let after = &msg[pos + prefix.len()..];
            let num_str: String = after
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(secs) = num_str.parse::<f64>() {
                if secs.is_finite() && secs >= 0.0 {
                    let millis = Duration::from_secs_f64(secs).as_millis();
                    if let Ok(value) = u64::try_from(millis) {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

fn pseudo_jitter_seed(attempt: u32) -> f64 {
    let x = attempt.wrapping_mul(2_654_435_761);
    (x as f64 / u32::MAX as f64).clamp(0.0, 1.0)
}

fn class_backoff_ms(attempt: u32, class: FailureClass) -> Option<u64> {
    let schedule: &[u64] = match class {
        FailureClass::EngineOverloaded => &[
            1_000, 2_000, 4_000, 8_000, 15_000, 30_000, 30_000, 30_000, 30_000, 30_000,
        ],
        FailureClass::AccountRateLimited => {
            &[5_000, 15_000, 30_000, 60_000, 60_000, 60_000, 60_000, 60_000, 60_000, 60_000]
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

    let drop_count = non_system.len() / 2;
    let indices_to_remove: Vec<usize> = non_system[..drop_count].to_vec();

    for &idx in indices_to_remove.iter().rev() {
        messages.remove(idx);
    }

    drop_count
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

pub const TRANSIENT_RETRY_FLOOR: u32 = 4;

pub const TRANSPORT_RETRY_CAP: u32 = 2;

const STREAM_BACKOFF_CEILING_MS: u64 = 10_000;

pub struct ReliableProvider {
    providers: Vec<(String, Arc<dyn Provider>)>,
    max_retries: u32,
    base_backoff_ms: u64,

    api_keys: Vec<String>,
    key_index: AtomicUsize,

    model_fallbacks: HashMap<String, Vec<String>>,

    counter: std::sync::Arc<crate::providers::core::retry::ReliabilityCounter>,

    rate_limiters: std::sync::Arc<crate::providers::core::rate_limit::RateLimiterMap<String>>,

    client_rate_limit_enabled: bool,

    engine_overload_max_retries: u32,

    account_rate_limit_max_retries: u32,

    transient_max_retries: u32,
}

impl ReliableProvider {
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
            api_keys: Vec::new(),
            key_index: AtomicUsize::new(0),
            model_fallbacks: HashMap::new(),
            counter: std::sync::Arc::new(crate::providers::core::retry::ReliabilityCounter::new()),
            rate_limiters: std::sync::Arc::new(
                crate::providers::core::rate_limit::RateLimiterMap::new(1_000_000.0, 1_000_000.0),
            ),
            client_rate_limit_enabled: false,
            engine_overload_max_retries: 10,
            account_rate_limit_max_retries: 5,
            transient_max_retries: max_retries.max(TRANSIENT_RETRY_FLOOR),
        }
    }

    pub fn with_rate_limit(mut self, capacity: f64, refill_per_sec: f64) -> Self {
        self.rate_limiters =
            std::sync::Arc::new(crate::providers::core::rate_limit::RateLimiterMap::new(
                capacity.max(0.0),
                refill_per_sec.max(0.0),
            ));
        self.client_rate_limit_enabled = capacity.is_finite()
            && refill_per_sec.is_finite()
            && capacity > 0.0
            && refill_per_sec > 0.0;
        self
    }

    pub fn with_client_rate_limit_enabled(mut self, enabled: bool) -> Self {
        self.client_rate_limit_enabled = enabled;
        self
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

    pub fn rate_limiters(
        &self,
    ) -> std::sync::Arc<crate::providers::core::rate_limit::RateLimiterMap<String>> {
        std::sync::Arc::clone(&self.rate_limiters)
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

    pub fn with_api_keys(mut self, keys: Vec<String>) -> Self {
        self.api_keys = keys;
        self
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

    fn rotate_key(&self) -> Option<&str> {
        if self.api_keys.is_empty() {
            return None;
        }
        let idx = self.key_index.fetch_add(1, Ordering::Relaxed) % self.api_keys.len();
        Some(&self.api_keys[idx])
    }

    fn compute_backoff_for_class(
        &self,
        attempt: u32,
        err: &anyhow::Error,
        class: FailureClass,
    ) -> u64 {
        if let Some(retry_after) = parse_retry_after_ms(err) {
            return retry_after.min(60_000);
        }
        if let Some(ms) = class_backoff_ms(attempt, class) {
            return ms;
        }
        crate::providers::core::retry::exp_backoff(attempt, self.base_backoff_ms, 30_000, 0.25)
            .as_millis() as u64
    }

    async fn gate_rate_limit(&self, provider: &str) {
        if !self.client_rate_limit_enabled {
            return;
        }
        self.rate_limiters.wait(&provider.to_string(), 1.0).await;
    }

    fn effective_class_cap(&self, class: FailureClass) -> u32 {
        match class {
            FailureClass::EngineOverloaded => self.engine_overload_max_retries,
            FailureClass::AccountRateLimited => self.account_rate_limit_max_retries,
            FailureClass::Transient => self.transient_max_retries,
            FailureClass::NonRetryable => self.max_retries,
        }
    }

    fn record_idempotency(
        &self,
        provider: &str,
        model: &str,
        messages: &serde_json::Value,
        tools: &serde_json::Value,
    ) -> crate::providers::core::idempotency::IdempotencyKey {
        let key =
            crate::providers::core::idempotency::fingerprint_json(provider, model, messages, tools);
        tracing::debug!(
            idempotency_key = %key.as_str(),
            provider,
            model,
            "dispatching provider request"
        );
        key
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

        if rate_limited && !is_non_retryable_rate_limit(err) {
            if let Some(new_key) = self.rotate_key() {
                tracing::warn!(
                    provider = provider_name,
                    error = %error_detail,
                    "Rate limited; key rotation selected key ending ...{} \
                     but cannot apply (Provider trait has no set_api_key). \
                     Retrying with original key.",
                    &new_key[new_key.len().saturating_sub(4)..]
                );
            }
        }

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

        let wait = self
            .compute_backoff_for_class(attempt, err, class)
            .min(if matches!(class, FailureClass::Transient) {
                STREAM_BACKOFF_CEILING_MS
            } else {
                60_000
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

        let _idem_key = self.record_idempotency(
            self.providers
                .first()
                .map(|(n, _)| n.as_str())
                .unwrap_or("reliable"),
            model,
            &serde_json::json!({
                "system": system_prompt,
                "message": message,
                "temperature": temperature,
            }),
            &serde_json::Value::Null,
        );

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=outer_cap {
                    self.gate_rate_limit(provider_name).await;
                    match provider
                        .chat_with_system(system_prompt, message, current_model, temperature)
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
                                let primary = self
                                    .providers
                                    .first()
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("");
                                record_provider_fallback(
                                    primary,
                                    model,
                                    provider_name,
                                    current_model,
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

        let _idem_key = self.record_idempotency(
            self.providers
                .first()
                .map(|(n, _)| n.as_str())
                .unwrap_or("reliable"),
            model,
            &serde_json::to_value(&effective_messages).unwrap_or(serde_json::Value::Null),
            &serde_json::Value::Null,
        );

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=outer_cap {
                    self.gate_rate_limit(provider_name).await;
                    match provider
                        .chat_with_history(&effective_messages, current_model, temperature)
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
                                let primary = self
                                    .providers
                                    .first()
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("");
                                record_provider_fallback(
                                    primary,
                                    model,
                                    provider_name,
                                    current_model,
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

        let _idem_key = self.record_idempotency(
            self.providers
                .first()
                .map(|(n, _)| n.as_str())
                .unwrap_or("reliable"),
            model,
            &serde_json::to_value(&effective_messages).unwrap_or(serde_json::Value::Null),
            &serde_json::Value::Array(tools.to_vec()),
        );

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=outer_cap {
                    self.gate_rate_limit(provider_name).await;
                    match provider
                        .chat_with_tools(&effective_messages, tools, current_model, temperature)
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
                                let primary = self
                                    .providers
                                    .first()
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("");
                                record_provider_fallback(
                                    primary,
                                    model,
                                    provider_name,
                                    current_model,
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

        let _idem_key = self.record_idempotency(
            self.providers
                .first()
                .map(|(n, _)| n.as_str())
                .unwrap_or("reliable"),
            model,
            &serde_json::to_value(&effective_messages).unwrap_or(serde_json::Value::Null),
            &serde_json::to_value(request.tools).unwrap_or(serde_json::Value::Null),
        );

        let outer_cap = self.outer_retry_cap();
        let mut state = RetryState::default();

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=outer_cap {
                    self.gate_rate_limit(provider_name).await;
                    let req = ChatRequest {
                        messages: &effective_messages,
                        tools: request.tools,
                    };
                    match provider.chat(req, current_model, temperature).await {
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
                                let primary = self
                                    .providers
                                    .first()
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("");
                                record_provider_fallback(
                                    primary,
                                    model,
                                    provider_name,
                                    current_model,
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

        for (provider_name, provider) in &self.providers {
            if !provider.supports_streaming() || !options.enabled {
                continue;
            }

            if needs_tool_events && !provider.supports_streaming_tool_events() {
                continue;
            }

            let provider_arc = Arc::clone(provider);
            let provider_label = provider_name.clone();

            let current_model = self
                .model_chain(model)
                .first()
                .copied()
                .unwrap_or(model)
                .to_string();

            let messages_owned: Arc<Vec<ChatMessage>> = Arc::new(request.messages.to_vec());
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
            let session_label = crate::session::current_session_context()
                .map(|ctx| ctx.session_id)
                .unwrap_or_default();

            let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

            let _bg = crate::runtime::spawn_supervised(
                "providers.reliable.stream_chat_retry",
                async move {
                    let mut engine_overload_attempts: u32 = 0;
                    let mut rate_limit_attempts: u32 = 0;
                    let mut transient_attempts: u32 = 0;
                    let mut transport_attempts: u32 = 0;
                    let mut total_attempts: u32 = 0;

                    loop {
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
                        let mut stream = provider_arc.stream_chat(
                            req,
                            &current_model,
                            temperature,
                            options,
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
                                            | StreamEvent::Usage(_)
                                    ) {
                                        made_progress = true;
                                    }
                                    let is_final = matches!(ev, StreamEvent::Final);
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
                                let _ = tx
                                    .send(Err(StreamError::Provider(
                                        "Upstream stream produced no events".to_string(),
                                    )))
                                    .await;
                            } else {
                                let _ = tx.send(Ok(StreamEvent::Final)).await;
                            }
                            return;
                        };

                        if made_progress {
                            let _ = tx.send(Err(err)).await;
                            return;
                        }

                        let err_string = err.to_string();
                        let anyhow_err = anyhow::anyhow!("{}", err_string);
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
                                        let _ = tx
                                            .send(Err(StreamError::Provider(summary)))
                                            .await;
                                        return;
                                    }
                                }
                                (transient_attempts, transient_cap, RetryClass::Transient)
                            }
                            FailureClass::NonRetryable => {
                                let _ = tx.send(Err(err)).await;
                                return;
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
                            let _ = tx.send(Err(StreamError::Provider(summary))).await;
                            return;
                        }

                        let wait_ms_raw = if let Some(ms) = parse_retry_after_ms(&anyhow_err) {
                            ms.min(60_000)
                        } else if let Some(ms) =
                            class_backoff_ms(total_attempts, class)
                        {
                            ms
                        } else {
                            crate::providers::core::retry::exp_backoff(
                                total_attempts,
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

                        let sleep_dur = Duration::from_millis(wait_ms);
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
                                () = tokio::time::sleep(sleep_dur) => {}
                            }
                        } else {
                            tokio::time::sleep(sleep_dur).await;
                        }

                        total_attempts = total_attempts.saturating_add(1);
                    }
                },
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

        for (provider_name, provider) in &self.providers {
            if !provider.supports_streaming() || !options.enabled {
                continue;
            }

            let provider_clone = provider_name.clone();

            let current_model = match self.model_chain(model).first() {
                Some(m) => (*m).to_string(),
                None => model.to_string(),
            };

            let stream = provider.stream_chat_with_system(
                system_prompt,
                message,
                &current_model,
                temperature,
                options,
            );

            let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

            let _bg = crate::runtime::spawn_supervised(
                "providers.reliable.stream_chat_with_system",
                async move {
                    let mut stream = stream;
                    while let Some(chunk) = stream.next().await {
                        if let Err(ref e) = chunk {
                            tracing::warn!(
                                provider = provider_clone,
                                model = current_model,
                                "Streaming error: {e}"
                            );
                        }
                        if tx.send(chunk).await.is_err() {
                            break;
                        }
                    }
                },
            );

            return stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|chunk| (chunk, rx))
            })
            .boxed();
        }

        stream::once(async move {
            Err(super::traits::StreamError::Provider(
                "No provider supports streaming".to_string(),
            ))
        })
        .boxed()
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {

        for (provider_name, provider) in &self.providers {
            if !provider.supports_streaming() || !options.enabled {
                continue;
            }

            let provider_clone = provider_name.clone();

            let current_model = match self.model_chain(model).first() {
                Some(m) => (*m).to_string(),
                None => model.to_string(),
            };

            let stream =
                provider.stream_chat_with_history(messages, &current_model, temperature, options);

            let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

            let _bg = crate::runtime::spawn_supervised(
                "providers.reliable.stream_chat_with_system",
                async move {
                    let mut stream = stream;
                    while let Some(chunk) = stream.next().await {
                        if let Err(ref e) = chunk {
                            tracing::warn!(
                                provider = provider_clone,
                                model = current_model,
                                "Streaming error: {e}"
                            );
                        }
                        if tx.send(chunk).await.is_err() {
                            break;
                        }
                    }
                },
            );

            return stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|chunk| (chunk, rx))
            })
            .boxed();
        }

        stream::once(async move {
            Err(super::traits::StreamError::Provider(
                "No provider supports streaming".to_string(),
            ))
        })
        .boxed()
    }
}
