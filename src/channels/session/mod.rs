// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod backend;
pub mod sqlite;
pub mod store;

use std::sync::Arc;

static GLOBAL_SESSION_BACKEND: parking_lot::RwLock<Option<Arc<dyn backend::SessionBackend>>> =
    parking_lot::RwLock::new(None);

pub fn set_global_session_backend(backend: Arc<dyn backend::SessionBackend>) {
    *GLOBAL_SESSION_BACKEND.write() = Some(backend);
}

pub fn global_session_backend() -> Option<Arc<dyn backend::SessionBackend>> {
    GLOBAL_SESSION_BACKEND.read().clone()
}
