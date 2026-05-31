// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::future::Future;
use std::time::Duration;

use crate::error::{ErrorCategory, ErrorClassification};
use crate::providers::core::retry::{exp_backoff, RetryBudget};

#[derive(Debug, thiserror::Error)]
pub enum RetryExhausted {
    #[error("retry policy exhausted budget without any attempt being executed (max_attempts={max_attempts}, max_elapsed={max_elapsed:?})")]
    BudgetEmpty {
        max_attempts: u32,
        max_elapsed: Duration,
    },
}

impl ErrorClassification for RetryExhausted {
    fn category(&self) -> ErrorCategory {
        ErrorCategory::Internal
    }
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_ms: u64,
    pub max_ms: u64,
    pub jitter: f64,
    pub max_elapsed: Duration,
    pub respect_retry_after: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::network()
    }
}

impl RetryPolicy {
    pub fn never() -> Self {
        Self {
            max_attempts: 1,
            base_ms: 0,
            max_ms: 0,
            jitter: 0.0,
            max_elapsed: Duration::from_millis(0),
            respect_retry_after: false,
        }
    }

    pub fn network() -> Self {
        Self {
            max_attempts: 4,
            base_ms: 250,
            max_ms: 5_000,
            jitter: 0.3,
            max_elapsed: Duration::from_secs(30),
            respect_retry_after: true,
        }
    }

    pub fn http() -> Self {
        Self {
            max_attempts: 4,
            base_ms: 300,
            max_ms: 8_000,
            jitter: 0.3,
            max_elapsed: Duration::from_secs(60),
            respect_retry_after: true,
        }
    }

    pub fn sqlite_busy() -> Self {
        Self {
            max_attempts: 6,
            base_ms: 10,
            max_ms: 1_000,
            jitter: 0.2,
            max_elapsed: Duration::from_secs(5),
            respect_retry_after: false,
        }
    }

    pub fn embedding() -> Self {
        Self {
            max_attempts: 4,
            base_ms: 500,
            max_ms: 8_000,
            jitter: 0.3,
            max_elapsed: Duration::from_secs(45),
            respect_retry_after: true,
        }
    }

    pub fn polling(max_polls: u32, interval_ms: u64) -> Self {
        Self {
            max_attempts: max_polls,
            base_ms: interval_ms,
            max_ms: interval_ms,
            jitter: 0.0,
            max_elapsed: Duration::from_secs(u64::from(max_polls).saturating_mul(interval_ms.max(1)) / 1000 + 60),
            respect_retry_after: false,
        }
    }

    pub fn with_max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = n;
        self
    }

    pub fn with_max_elapsed(mut self, d: Duration) -> Self {
        self.max_elapsed = d;
        self
    }
}

pub async fn retry<F, Fut, T, E>(policy: &RetryPolicy, mut op: F) -> Result<T, E>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: ErrorClassification + From<RetryExhausted>,
{
    let mut budget = RetryBudget::new(policy.max_attempts, policy.max_elapsed);
    let mut last_err: Option<E> = None;
    let mut attempt: u32 = 0;

    loop {
        if !budget.try_consume() {
            break;
        }
        attempt += 1;
        match op(attempt).await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if !err.is_retryable() {
                    return Err(err);
                }
                let hint = err.retry_after_hint();
                if attempt >= policy.max_attempts {
                    return Err(err);
                }
                last_err = Some(err);
                let mut delay =
                    exp_backoff(attempt - 1, policy.base_ms, policy.max_ms, policy.jitter);
                if policy.respect_retry_after {
                    if let Some(h) = hint {
                        if h > delay {
                            delay = h;
                        }
                    }
                }
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    match last_err {
        Some(err) => Err(err),
        None => Err(E::from(RetryExhausted::BudgetEmpty {
            max_attempts: policy.max_attempts,
            max_elapsed: policy.max_elapsed,
        })),
    }
}

pub async fn retry_simple<F, Fut, T, E>(op: F) -> Result<T, E>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: ErrorClassification + From<RetryExhausted>,
{
    retry(&RetryPolicy::network(), op).await
}
