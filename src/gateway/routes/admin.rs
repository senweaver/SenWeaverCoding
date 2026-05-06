// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Admin resource — cron scheduler, skills management, integrations.
//!
//! placeholder sub-router that will absorb the following
//! handlers currently in [`crate::gateway::api`]:
//!
//! * `handle_api_cron_list` / `handle_api_cron_add` / `handle_api_cron_runs`
//!   / `handle_api_cron_patch` / `handle_api_cron_delete`
//! * `handle_api_cron_settings_get` / `handle_api_cron_settings_patch`
//! * `handle_api_skills_get` / `handle_api_skills_put`
//! * `handle_api_integrations` / `handle_api_integrations_settings`
//!
//! Until the follow-up sprint moves the handler bodies, this
//! module exposes `admin_router()` — an empty [`axum::Router`] that the
//! `build_router()` aggregator can mount safely.  The real routes
//! remain registered via `api.rs` in the meantime; the builder calls
//! `admin_router().merge(...)` so the mount points are live and the
//! refactor becomes a handler-move rather than an aggregator change.

use axum::Router;

pub fn admin_router() -> Router {
    Router::new()
}
