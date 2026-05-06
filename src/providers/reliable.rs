// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::Provider;
use super::traits::{
    ChatMessage, ChatRequest, ChatResponse, StreamChunk, StreamEvent, StreamOptions, StreamResult,
};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

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

fn failure_reason(rate_limited: bool, non_retryable: bool) -> &'static str {
    if rate_limited && non_retryable {
        "rate_limited_non_retryable"
    } else if rate_limited {
        "rate_limited"
    } else if non_retryable {
        "non_retryable"
    } else {
        "retryable"
    }
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

pub struct ReliableProvider {
    providers: Vec<(String, Box<dyn Provider>)>,
    max_retries: u32,
    base_backoff_ms: u64,

    api_keys: Vec<String>,
    key_index: AtomicUsize,

    model_fallbacks: HashMap<String, Vec<String>>,

    counter: std::sync::Arc<crate::providers::core::retry::ReliabilityCounter>,

    rate_limiters: std::sync::Arc<crate::providers::core::rate_limit::RateLimiterMap<String>>,
}

impl ReliableProvider {
    pub fn new(
        providers: Vec<(String, Box<dyn Provider>)>,
        max_retries: u32,
        base_backoff_ms: u64,
    ) -> Self {
        Self {
            providers,
            max_retries,
            base_backoff_ms: base_backoff_ms.max(50),
            api_keys: Vec::new(),
            key_index: AtomicUsize::new(0),
            model_fallbacks: HashMap::new(),
            counter: std::sync::Arc::new(crate::providers::core::retry::ReliabilityCounter::new()),
            rate_limiters: std::sync::Arc::new(
                crate::providers::core::rate_limit::RateLimiterMap::new(1_000.0, 1_000.0),
            ),
        }
    }

    pub fn with_rate_limit(mut self, capacity: f64, refill_per_sec: f64) -> Self {
        self.rate_limiters =
            std::sync::Arc::new(crate::providers::core::rate_limit::RateLimiterMap::new(
                capacity.max(0.0),
                refill_per_sec.max(0.0),
            ));
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

    fn compute_backoff(&self, base: u64, err: &anyhow::Error) -> u64 {
        if let Some(retry_after) = parse_retry_after_ms(err) {

            retry_after.min(30_000).max(base)
        } else {
            base
        }
    }

    fn compute_backoff_exp(&self, attempt: u32, err: &anyhow::Error) -> u64 {
        if let Some(retry_after) = parse_retry_after_ms(err) {
            return retry_after.min(30_000);
        }
        crate::providers::core::retry::exp_backoff(attempt, self.base_backoff_ms, 30_000, 0.25)
            .as_millis() as u64
    }

    async fn gate_rate_limit(&self, provider: &str) {
        self.rate_limiters.wait(&provider.to_string(), 1.0).await;
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
        if is_rate_limited(err) {
            return crate::providers::core::retry::RetryClass::RateLimited;
        }
        crate::providers::core::retry::RetryClass::Transient
    }
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

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=self.max_retries {
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
                                    self.max_retries + 1,
                                    "non_retryable",
                                    &error_detail,
                                );
                                anyhow::bail!(
                                    "Request exceeds model context window. Attempts:\n{}",
                                    failures.join("\n")
                                );
                            }

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);

                            push_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                &error_detail,
                            );

                            if rate_limited && !non_retryable_rate_limit {
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

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    error = %error_detail,
                                    "Non-retryable error, moving on"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                let wait = self.compute_backoff_exp(attempt, &e);
                                let retry_class = Self::classify_retry(&e);
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt = attempt + 1,
                                    backoff_ms = wait,
                                    reason = failure_reason,
                                    retry_class = ?retry_class,
                                    error = %error_detail,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
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

        anyhow::bail!(
            "All providers/models failed. Attempts:\n{}",
            failures.join("\n")
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

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=self.max_retries {
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
                                    self.max_retries + 1,
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

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);

                            push_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                &error_detail,
                            );

                            if rate_limited && !non_retryable_rate_limit {
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

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    error = %error_detail,
                                    "Non-retryable error, moving on"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                let wait = self.compute_backoff_exp(attempt, &e);
                                let retry_class = Self::classify_retry(&e);
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt = attempt + 1,
                                    backoff_ms = wait,
                                    reason = failure_reason,
                                    retry_class = ?retry_class,
                                    error = %error_detail,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
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

        anyhow::bail!(
            "All providers/models failed. Attempts:\n{}",
            failures.join("\n")
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

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=self.max_retries {
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
                                    self.max_retries + 1,
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

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);

                            push_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                &error_detail,
                            );

                            if rate_limited && !non_retryable_rate_limit {
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

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    error = %error_detail,
                                    "Non-retryable error, moving on"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                let wait = self.compute_backoff_exp(attempt, &e);
                                let retry_class = Self::classify_retry(&e);
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt = attempt + 1,
                                    backoff_ms = wait,
                                    reason = failure_reason,
                                    retry_class = ?retry_class,
                                    error = %error_detail,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
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

        anyhow::bail!(
            "All providers/models failed. Attempts:\n{}",
            failures.join("\n")
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

        for current_model in &models {
            for (provider_name, provider) in &self.providers {
                for attempt in 0..=self.max_retries {
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
                                    self.max_retries + 1,
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

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);

                            push_failure(
                                &mut failures,
                                provider_name,
                                current_model,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                &error_detail,
                            );

                            if rate_limited && !non_retryable_rate_limit {
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

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    error = %error_detail,
                                    "Non-retryable error, moving on"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                let wait = self.compute_backoff_exp(attempt, &e);
                                let retry_class = Self::classify_retry(&e);
                                tracing::warn!(
                                    provider = provider_name,
                                    model = *current_model,
                                    attempt = attempt + 1,
                                    backoff_ms = wait,
                                    reason = failure_reason,
                                    retry_class = ?retry_class,
                                    error = %error_detail,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
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

        anyhow::bail!(
            "All providers/models failed. Attempts:\n{}",
            failures.join("\n")
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

            let provider_clone = provider_name.clone();

            let current_model = self
                .model_chain(model)
                .first()
                .copied()
                .unwrap_or(model)
                .to_string();

            let req = ChatRequest {
                messages: request.messages,
                tools: request.tools,
            };
            let stream = provider.stream_chat(req, &current_model, temperature, options);
            let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

            let _bg = crate::runtime::spawn_supervised(
                "providers.reliable.stream_chat_with_tools",
                async move {
                    let mut stream = stream;
                    while let Some(event) = stream.next().await {
                        if let Err(ref e) = event {
                            tracing::warn!(
                                provider = provider_clone,
                                model = current_model,
                                "Streaming error: {e}"
                            );
                        }
                        if tx.send(event).await.is_err() {
                            break;
                        }
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
