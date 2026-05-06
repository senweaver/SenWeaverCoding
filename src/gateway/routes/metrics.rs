// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Metrics resource — `/metrics` Prometheus exposition.
//!
//! D2.4 placeholder for the upcoming `handle_api_metrics`
//! move from [`crate::gateway::api`].  D5.1 extends the
//! metric list to 18 items; the handler will stay small (register +
//! gather + encode), so moving it here is a clean extraction.

use axum::Router;

pub fn metrics_router() -> Router {
    Router::new()
}
