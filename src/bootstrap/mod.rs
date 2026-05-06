// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Bootstrap module — global session state management.
//
// Mirrors claude-code's `bootstrap/state.ts`: a single process-wide state
// struct that tracks session identity, cost counters, telemetry handles,
// model usage, and ephemeral per-session flags. The state is initialised
// once at startup and accessed through thread-safe accessor functions.

pub mod app_context;
pub mod state;

pub use app_context::AppContext;
pub use state::{
    BootstrapState, SessionState, get_cwd, get_project_root, get_session_id, get_state, init_state,
    reset_state, set_cwd, try_get_state,
};
