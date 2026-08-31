// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

const COOLDOWN_BASE: Duration = Duration::from_secs(30);
const COOLDOWN_MAX: Duration = Duration::from_secs(300);
const COOLDOWN_CAPTCHA: Duration = Duration::from_secs(600);
const EWMA_ALPHA: f64 = 0.3;
const FAILURES_BEFORE_COOLDOWN: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Timeout,
    Network,
    Captcha,
    Other,
}

#[derive(Debug, Clone, Default)]
struct EngineHealth {
    consecutive_failures: u32,
    ewma_latency_ms: Option<f64>,
    cooldown_until: Option<Instant>,
}

fn registry() -> &'static RwLock<HashMap<&'static str, EngineHealth>> {
    static REGISTRY: OnceLock<RwLock<HashMap<&'static str, EngineHealth>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn record_success(engine_id: &'static str, elapsed: Duration) {
    let mut guard = registry().write();
    let entry = guard.entry(engine_id).or_default();
    entry.consecutive_failures = 0;
    entry.cooldown_until = None;
    let sample = elapsed.as_millis() as f64;
    entry.ewma_latency_ms = Some(match entry.ewma_latency_ms {
        Some(prev) => prev * (1.0 - EWMA_ALPHA) + sample * EWMA_ALPHA,
        None => sample,
    });
}

pub fn record_failure(engine_id: &'static str, kind: FailureKind) {
    let mut guard = registry().write();
    let entry = guard.entry(engine_id).or_default();
    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
    let cooldown = match kind {
        FailureKind::Captcha => Some(COOLDOWN_CAPTCHA),
        FailureKind::Timeout | FailureKind::Network => {
            if entry.consecutive_failures >= FAILURES_BEFORE_COOLDOWN {
                let exponent = entry
                    .consecutive_failures
                    .saturating_sub(FAILURES_BEFORE_COOLDOWN)
                    .min(4);
                let scaled = COOLDOWN_BASE.saturating_mul(1_u32 << exponent);
                Some(scaled.min(COOLDOWN_MAX))
            } else {
                None
            }
        }
        FailureKind::Other => None,
    };
    if let Some(cooldown) = cooldown {
        entry.cooldown_until = Some(Instant::now() + cooldown);
        tracing::info!(
            target: "tools.web_search.health",
            engine = engine_id,
            failures = entry.consecutive_failures,
            cooldown_secs = cooldown.as_secs(),
            kind = ?kind,
            "engine placed in cooldown"
        );
    }
}

pub fn is_cooling_down(engine_id: &str) -> bool {
    let guard = registry().read();
    guard
        .get(engine_id)
        .and_then(|entry| entry.cooldown_until)
        .is_some_and(|until| Instant::now() < until)
}

pub fn ewma_latency_ms(engine_id: &str) -> Option<f64> {
    registry()
        .read()
        .get(engine_id)
        .and_then(|entry| entry.ewma_latency_ms)
}

pub fn classify_failure(message: &str) -> FailureKind {
    let lower = message.to_ascii_lowercase();
    if lower.contains("captcha")
        || lower.contains("robot check")
        || lower.contains("are you human")
        || lower.contains("verify you are not a robot")
    {
        return FailureKind::Captcha;
    }
    if lower.contains("engine timeout after") || lower.contains("timed out") || lower.contains("timeout") {
        return FailureKind::Timeout;
    }
    if lower.contains("error sending request")
        || lower.contains("connection reset")
        || lower.contains("connection closed")
        || lower.contains("broken pipe")
        || lower.contains("tls handshake")
        || lower.contains("dns")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        return FailureKind::Network;
    }
    FailureKind::Other
}
