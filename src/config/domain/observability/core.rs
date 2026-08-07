// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObservabilityConfig {

    pub backend: String,

    #[serde(default)]
    pub otel_endpoint: Option<String>,

    #[serde(default)]
    pub otel_service_name: Option<String>,

    #[serde(default = "default_runtime_trace_mode")]
    pub runtime_trace_mode: String,

    #[serde(default = "default_runtime_trace_path")]
    pub runtime_trace_path: String,

    #[serde(default = "default_runtime_trace_max_entries")]
    pub runtime_trace_max_entries: usize,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            backend: "none".into(),
            otel_endpoint: None,
            otel_service_name: None,
            runtime_trace_mode: default_runtime_trace_mode(),
            runtime_trace_path: default_runtime_trace_path(),
            runtime_trace_max_entries: default_runtime_trace_max_entries(),
        }
    }
}

impl ObservabilityConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let known = [
            "none",
            "noop",
            "log",
            "verbose",
            "prometheus",
            "otel",
            "opentelemetry",
            "otlp",
        ];
        for token in self.backend.split(',').map(str::trim) {
            if token.is_empty() {
                continue;
            }
            if !known.contains(&token) {
                errors.push(format!(
                    "observability.backend contains unknown backend '{token}'; \
                     known backends: {}",
                    known.join(", ")
                ));
            }
        }
        if self
            .backend
            .split(',')
            .map(str::trim)
            .any(|t| matches!(t, "otel" | "opentelemetry" | "otlp"))
        {
            let endpoint = self
                .otel_endpoint
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if endpoint.is_none() {
                errors.push(
                    "observability.backend otel/opentelemetry/otlp requires a non-empty otel_endpoint"
                        .into(),
                );
            }
        }
        let valid_trace_modes = ["none", "rolling", "full"];
        if !valid_trace_modes.contains(&self.runtime_trace_mode.as_str()) {
            errors.push(format!(
                "observability.runtime_trace_mode '{}' must be one of {:?}",
                self.runtime_trace_mode, valid_trace_modes
            ));
        }
        if self.runtime_trace_mode == "rolling" && self.runtime_trace_max_entries == 0 {
            errors.push("observability.runtime_trace_max_entries must be > 0 when rolling".into());
        }
        errors
    }
}

pub(crate) fn default_runtime_trace_mode() -> String {
    "none".to_string()
}
pub(crate) fn default_runtime_trace_path() -> String {
    "state/runtime-trace.jsonl".to_string()
}
pub(crate) fn default_runtime_trace_max_entries() -> usize {
    200
}
