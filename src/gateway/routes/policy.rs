// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Policy resource — autonomy, sandbox, prompt-guard, secrets.
//!
//! D2.4 placeholder for the upcoming handler move from
//! [`crate::gateway::api`]:
//!
//! * `handle_api_doctor` (effective policy dump)
//! * Autonomy level + sandbox backend introspection.
//! * Prompt-guard rule list (D6.4 YAML source).
//! * Secrets inventory (redacted).

use axum::Router;

pub fn policy_router() -> Router {
    Router::new()
}
