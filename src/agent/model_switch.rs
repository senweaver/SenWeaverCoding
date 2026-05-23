// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

#[allow(clippy::type_complexity)]
pub type ModelSwitchCallback = Arc<parking_lot::Mutex<Option<(String, String)>>>;

#[derive(Clone, Default)]
pub(crate) struct ModelSwitchState {
    pub switch: Arc<parking_lot::Mutex<Option<(String, String)>>>,
}

tokio::task_local! {
    static MODEL_SWITCH_STATE: ModelSwitchState;
}

pub fn get_model_switch_state() -> ModelSwitchCallback {
    MODEL_SWITCH_STATE
        .try_with(|s| Arc::clone(&s.switch))
        .unwrap_or_else(|_| Arc::new(parking_lot::Mutex::new(None)))
}

pub fn clear_model_switch_request() {
    if let Ok(state) = MODEL_SWITCH_STATE.try_with(|s| Arc::clone(&s.switch)) {
        let mut guard = state.lock();
        *guard = None;
    }
}

pub async fn scope_model_switch<F, R>(f: F) -> R
where
    F: std::future::Future<Output = R>,
{
    let state = ModelSwitchState::default();
    MODEL_SWITCH_STATE.scope(state, f).await
}

#[derive(Debug)]
pub(crate) struct ModelSwitchRequested {
    pub provider: String,
    pub model: String,
}

impl std::fmt::Display for ModelSwitchRequested {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "model switch requested to {} {}",
            self.provider, self.model
        )
    }
}

impl std::error::Error for ModelSwitchRequested {}

pub(crate) fn is_model_switch_requested(err: &anyhow::Error) -> Option<(String, String)> {
    err.chain()
        .filter_map(|source| source.downcast_ref::<ModelSwitchRequested>())
        .map(|e| (e.provider.clone(), e.model.clone()))
        .next()
}
