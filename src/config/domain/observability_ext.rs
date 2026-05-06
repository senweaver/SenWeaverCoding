// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Observability extensions — OTLP, metrics, sampling.
//!
//! Split of P6.1: observability concerns that extend `ObservabilityConfig`
//! with runtime-tunable knobs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObservabilityExtras {

    #[serde(default = "default_trace_sample_rate")]
    pub trace_sample_rate: f64,

    #[serde(default = "default_turn_latency_buckets")]
    pub turn_latency_buckets: Vec<f64>,

    #[serde(default = "default_true")]
    pub per_tool_span: bool,

    #[serde(default = "default_event_history_size")]
    pub event_history_size: usize,
}

fn default_trace_sample_rate() -> f64 {
    1.0
}
fn default_turn_latency_buckets() -> Vec<f64> {
    vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]
}
fn default_true() -> bool {
    true
}
fn default_event_history_size() -> usize {
    1000
}

impl Default for ObservabilityExtras {
    fn default() -> Self {
        Self {
            trace_sample_rate: default_trace_sample_rate(),
            turn_latency_buckets: default_turn_latency_buckets(),
            per_tool_span: default_true(),
            event_history_size: default_event_history_size(),
        }
    }
}

impl ObservabilityExtras {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !(0.0..=1.0).contains(&self.trace_sample_rate) {
            errors.push(format!(
                "observability.trace_sample_rate must be in [0.0, 1.0], got {}",
                self.trace_sample_rate
            ));
        }
        if self.turn_latency_buckets.is_empty() {
            errors.push("observability.turn_latency_buckets must have >= 1 bucket".into());
        }
        if !self.turn_latency_buckets.windows(2).all(|w| w[0] < w[1]) {
            errors.push("observability.turn_latency_buckets must be strictly increasing".into());
        }
        if self.event_history_size == 0 {
            errors.push("observability.event_history_size must be > 0".into());
        }
        errors
    }
}
