// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod app_context;
pub mod state;

pub use app_context::AppContext;
pub use state::{
    BootstrapState, SessionState, get_cwd, get_project_root, get_session_id, get_state, init_state,
    reset_state, set_cwd, try_get_state,
};
