// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `AppContext` — a typed handle that proves initialization.
//!
//! Instead of having every command call `get_state()` (which panics if the
//! global isn't initialized) or wrap everything in `catch_unwind`, we hand
//! initialized commands an `AppContext` that **by construction** guarantees
//! both bootstrap state and the service container are live.

use super::state::BootstrapState;

#[derive(Clone)]
pub struct AppContext {
    pub state: BootstrapState,
    pub services: &'static crate::services::ServiceContainer,
}

impl AppContext {

    pub fn capture() -> Option<Self> {
        let state = crate::bootstrap::try_get_state()?.clone();
        let services = crate::services::try_get_services()?;
        Some(Self { state, services })
    }

    pub fn state(&self) -> &BootstrapState {
        &self.state
    }

    pub fn services(&self) -> &crate::services::ServiceContainer {
        self.services
    }
}
