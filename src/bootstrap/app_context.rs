// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
