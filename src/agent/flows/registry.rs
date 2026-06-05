// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::{Arc, OnceLock, RwLock};

use super::checkpoint::CheckpointStore;
use super::traits::AgentHandle;

static GLOBAL_AGENT_HANDLE: OnceLock<RwLock<Option<Arc<dyn AgentHandle>>>> = OnceLock::new();
static GLOBAL_CHECKPOINT_STORE: OnceLock<Arc<CheckpointStore>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<Arc<dyn AgentHandle>>> {
    GLOBAL_AGENT_HANDLE.get_or_init(|| RwLock::new(None))
}

pub fn set_global_agent_handle(handle: Arc<dyn AgentHandle>) {
    let cell = slot();
    let mut guard = cell.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(handle);
}

pub fn global_agent_handle() -> Option<Arc<dyn AgentHandle>> {
    slot().read().ok().and_then(|g| g.clone())
}

#[doc(hidden)]
pub fn clear_global_agent_handle_for_tests() {
    if let Some(cell) = GLOBAL_AGENT_HANDLE.get() {
        let _ = cell.write().map(|mut g| *g = None);
    }
}

pub fn global_checkpoint_store() -> Arc<CheckpointStore> {
    GLOBAL_CHECKPOINT_STORE
        .get_or_init(|| Arc::new(CheckpointStore::default()))
        .clone()
}
