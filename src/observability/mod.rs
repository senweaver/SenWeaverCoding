// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod agent_metrics;
pub mod dora;
pub mod log;
pub mod multi;
pub mod noop;
#[cfg(feature = "observability-otel")]
pub mod otel;

pub mod otlp_schema;

pub mod subsystem_metrics;

pub mod code_intel_metrics;

pub mod session_write_mode_metrics;

pub mod coordination_metrics;

pub mod scheduler_metrics;

pub mod token_saver_metrics;
pub mod tui_metrics;
#[cfg(feature = "observability-prometheus")]
pub mod prometheus;
pub mod redact_layer;
pub mod runtime_trace;
pub mod traits;
pub mod verbose;

pub mod views;
pub use self::log::LogObserver;
pub use self::multi::MultiObserver;
pub use noop::NoopObserver;
#[cfg(feature = "observability-otel")]
pub use otel::OtelObserver;
#[cfg(feature = "observability-prometheus")]
pub use prometheus::PrometheusObserver;
pub use traits::{Observer, ObserverEvent};

use std::sync::{Arc, OnceLock};

static GLOBAL_OBSERVER: OnceLock<Arc<dyn Observer>> = OnceLock::new();

pub fn set_global_observer(observer: Arc<dyn Observer>) -> Arc<dyn Observer> {
    if GLOBAL_OBSERVER.get().is_some() {
        tracing::warn!(
            "set_global_observer called after observer was already installed; \
             the new observer is being ignored (singleton)"
        );
    }
    GLOBAL_OBSERVER.get_or_init(|| observer).clone()
}

pub fn global_observer() -> Option<Arc<dyn Observer>> {
    GLOBAL_OBSERVER.get().cloned()
}
pub use verbose::VerboseObserver;

use crate::config::ObservabilityConfig;

fn create_single_observer(token: &str, config: &ObservabilityConfig) -> Box<dyn Observer> {
    let _ = &config;
    match token {
        "log" => Box::new(LogObserver::new()),
        "verbose" => Box::new(VerboseObserver::new()),
        "prometheus" => {
            #[cfg(feature = "observability-prometheus")]
            {
                Box::new(PrometheusObserver::new())
            }
            #[cfg(not(feature = "observability-prometheus"))]
            {
                tracing::warn!(
                    "Prometheus backend requested but this build was compiled without `observability-prometheus`; falling back to noop."
                );
                Box::new(NoopObserver)
            }
        }
        "otel" | "opentelemetry" | "otlp" => {
            #[cfg(feature = "observability-otel")]
            match OtelObserver::new(
                config.otel_endpoint.as_deref(),
                config.otel_service_name.as_deref(),
            ) {
                Ok(obs) => {
                    tracing::info!(
                        endpoint = config
                            .otel_endpoint
                            .as_deref()
                            .unwrap_or("http://localhost:4318"),
                        "OpenTelemetry observer initialized"
                    );
                    Box::new(obs)
                }
                Err(e) => {
                    tracing::error!("Failed to create OTel observer: {e}. Falling back to noop.");
                    Box::new(NoopObserver)
                }
            }
            #[cfg(not(feature = "observability-otel"))]
            {
                tracing::warn!(
                    "OpenTelemetry backend requested but this build was compiled without `observability-otel`; falling back to noop."
                );
                Box::new(NoopObserver)
            }
        }
        "none" | "noop" => Box::new(NoopObserver),
        _ => {
            tracing::warn!(
                "Unknown observability backend '{}', falling back to noop",
                token
            );
            Box::new(NoopObserver)
        }
    }
}

pub fn create_observer(config: &ObservabilityConfig) -> Box<dyn Observer> {
    let tokens: Vec<&str> = config
        .backend
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();

    match tokens.len() {
        0 => Box::new(NoopObserver),
        1 => create_single_observer(tokens[0], config),
        _ => {
            let observers: Vec<Box<dyn Observer>> = tokens
                .iter()
                .map(|t| create_single_observer(t, config))
                .collect();
            Box::new(MultiObserver::new(observers))
        }
    }
}
