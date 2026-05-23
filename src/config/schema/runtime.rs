// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReliabilityConfig {

    #[serde(default = "default_provider_retries")]
    pub provider_retries: u32,

    #[serde(default = "default_provider_backoff_ms")]
    pub provider_backoff_ms: u64,

    #[serde(default)]
    pub fallback_providers: Vec<String>,

    #[serde(default, serialize_with = "crate::config::redact::redact_vec_string")]
    pub api_keys: Vec<String>,

    #[serde(default)]
    pub model_fallbacks: std::collections::HashMap<String, Vec<String>>,

    #[serde(default = "default_channel_backoff_secs")]
    pub channel_initial_backoff_secs: u64,

    #[serde(default = "default_channel_backoff_max_secs")]
    pub channel_max_backoff_secs: u64,

    #[serde(default = "default_scheduler_poll_secs")]
    pub scheduler_poll_secs: u64,

    #[serde(default = "default_scheduler_retries")]
    pub scheduler_retries: u32,

    #[serde(default = "default_engine_overload_max_retries")]
    pub engine_overload_max_retries: u32,

    #[serde(default = "default_account_rate_limit_max_retries")]
    pub account_rate_limit_max_retries: u32,

    #[serde(default = "default_transient_max_retries")]
    pub transient_max_retries: u32,

    #[serde(default = "default_client_llm_rate_limit_enabled")]
    pub client_llm_rate_limit_enabled: bool,
}

fn default_provider_retries() -> u32 {
    10
}

fn default_provider_backoff_ms() -> u64 {
    500
}

fn default_engine_overload_max_retries() -> u32 {
    10
}

fn default_account_rate_limit_max_retries() -> u32 {
    5
}

fn default_transient_max_retries() -> u32 {
    crate::providers::reliable::TRANSIENT_RETRY_FLOOR
}

fn default_client_llm_rate_limit_enabled() -> bool {
    false
}

fn default_channel_backoff_secs() -> u64 {
    2
}

fn default_channel_backoff_max_secs() -> u64 {
    60
}

fn default_scheduler_poll_secs() -> u64 {
    15
}

fn default_scheduler_retries() -> u32 {
    2
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            provider_retries: default_provider_retries(),
            provider_backoff_ms: default_provider_backoff_ms(),
            fallback_providers: Vec::new(),
            api_keys: Vec::new(),
            model_fallbacks: std::collections::HashMap::new(),
            channel_initial_backoff_secs: default_channel_backoff_secs(),
            channel_max_backoff_secs: default_channel_backoff_max_secs(),
            scheduler_poll_secs: default_scheduler_poll_secs(),
            scheduler_retries: default_scheduler_retries(),
            engine_overload_max_retries: default_engine_overload_max_retries(),
            account_rate_limit_max_retries: default_account_rate_limit_max_retries(),
            transient_max_retries: default_transient_max_retries(),
            client_llm_rate_limit_enabled: default_client_llm_rate_limit_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SchedulerConfig {

    #[serde(default = "default_scheduler_enabled")]
    pub enabled: bool,

    #[serde(default = "default_scheduler_max_tasks")]
    pub max_tasks: usize,

    #[serde(default = "default_scheduler_max_concurrent")]
    pub max_concurrent: usize,
}

fn default_scheduler_enabled() -> bool {
    true
}

fn default_scheduler_max_tasks() -> usize {
    64
}

fn default_scheduler_max_concurrent() -> usize {
    4
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: default_scheduler_enabled(),
            max_tasks: default_scheduler_max_tasks(),
            max_concurrent: default_scheduler_max_concurrent(),
        }
    }
}
