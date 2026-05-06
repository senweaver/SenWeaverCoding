// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// API service — mirrors claude-code-typescript-src`services/api/`.
// Wraps model API calls with retry logic, error categorization,
// usage accumulation, and streaming support.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCategory {

    ServerError,

    RateLimited,

    Overloaded,

    AuthError,

    InvalidRequest,

    ContextLengthExceeded,

    NetworkError,

    Timeout,

    Unknown,
}

pub fn categorize_api_error(status: u16, body: Option<&str>) -> ApiErrorCategory {
    match status {
        401 | 403 => ApiErrorCategory::AuthError,
        400 => {
            if let Some(b) = body {
                if b.contains("context_length") || b.contains("max_tokens") {
                    return ApiErrorCategory::ContextLengthExceeded;
                }
            }
            ApiErrorCategory::InvalidRequest
        }
        429 => ApiErrorCategory::RateLimited,
        529 => ApiErrorCategory::Overloaded,
        500..=599 => ApiErrorCategory::ServerError,
        0 => ApiErrorCategory::NetworkError,
        _ => ApiErrorCategory::Unknown,
    }
}

pub fn is_retryable(category: ApiErrorCategory) -> bool {
    matches!(
        category,
        ApiErrorCategory::ServerError
            | ApiErrorCategory::RateLimited
            | ApiErrorCategory::Overloaded
            | ApiErrorCategory::NetworkError
            | ApiErrorCategory::Timeout
    )
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

pub fn retry_delay(config: &RetryConfig, attempt: u32) -> Duration {
    let base =
        config.initial_delay.as_millis() as f64 * config.backoff_multiplier.powi(attempt as i32);
    let capped = base.min(config.max_delay.as_millis() as f64);
    let jitter = rand::random::<f64>() * 0.2 * capped;
    Duration::from_millis((capped + jitter) as u64)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl ApiUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    pub fn accumulate(&mut self, other: &ApiUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
    }
}
