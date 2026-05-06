// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Auth resource — pairing, OAuth callbacks, RBAC status.
//!
//! D2.4 placeholder for the upcoming handler move from
//! [`crate::gateway::api`]:
//!
//! * `handle_api_rbac_status`
//! * OAuth callback handlers (currently inlined in `api.rs`
//!   `build_router`).
//! * Pairing-token registration endpoints.

use axum::Router;

pub fn auth_router() -> Router {
    Router::new()
}
