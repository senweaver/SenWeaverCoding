// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Provider-health view model — tabular view of `HealthSignal`s
//! emitted by `providers::reliable` and consumed by the router /
//! supervisor.
//!
//! UI-agnostic: ratatui rendering lives in `crate::tui::panels`; the
//! Tauri-hosted desktop UI consumes the same shape via the gateway.
//! This module only holds the data.

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderHealthRow {
    pub provider: String,
    pub model: String,
    pub success_rate: f64,
    pub p95_latency_ms: u64,
    pub retries_per_req: f64,
    pub cost_per_1k_tok: f64,
}

impl ProviderHealthRow {

    pub fn is_unhealthy(&self) -> bool {
        self.success_rate < 0.80
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProviderHealthView {
    rows: Vec<ProviderHealthRow>,
}

impl ProviderHealthView {
    pub fn from_rows(mut rows: Vec<ProviderHealthRow>) -> Self {
        rows.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.model.cmp(&b.model)));
        Self { rows }
    }

    pub fn rows(&self) -> &[ProviderHealthRow] {
        &self.rows
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn unhealthy_count(&self) -> usize {
        self.rows.iter().filter(|r| r.is_unhealthy()).count()
    }

    pub fn header_line(&self) -> String {
        format!(
            "{} providers · {} unhealthy",
            self.row_count(),
            self.unhealthy_count(),
        )
    }
}
