// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use tokio::task_local;

#[derive(Debug, Clone)]
pub struct ModelSwitchRequest {
    pub provider: String,
    pub model: String,
}

impl ModelSwitchRequest {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
        }
    }

    pub fn to_tuple(&self) -> (String, String) {
        (self.provider.clone(), self.model.clone())
    }
}

#[allow(clippy::type_complexity)]
pub type ModelSwitchCallback = Arc<parking_lot::Mutex<Option<(String, String)>>>;

#[doc(hidden)]
task_local! {
    static MODEL_SWITCH_REQUEST: Arc<parking_lot::Mutex<Option<(String, String)>>>;
}

#[derive(Clone, Default)]
pub struct ModelSwitchGuard {
    _priv: (),
}

impl ModelSwitchGuard {

    pub fn new() -> Self {
        Self { _priv: () }
    }

    pub async fn scope<F, R>(&self, f: F) -> R
    where
        F: std::future::Future<Output = R>,
    {
        let storage: Arc<parking_lot::Mutex<Option<(String, String)>>> =
            Arc::new(parking_lot::Mutex::new(None));
        MODEL_SWITCH_REQUEST.scope(storage.clone(), f).await
    }

    pub fn get_callback() -> ModelSwitchCallback {
        MODEL_SWITCH_REQUEST
            .try_with(|s| Arc::clone(s))
            .unwrap_or_else(|_| Arc::new(parking_lot::Mutex::new(None)))
    }

    pub fn get() -> Option<(String, String)> {
        MODEL_SWITCH_REQUEST
            .try_with(|s| s.lock().clone())
            .ok()
            .flatten()
    }

    pub fn set(provider: impl Into<String>, model: impl Into<String>) {
        if let Ok(storage) = MODEL_SWITCH_REQUEST.try_with(|s| Arc::clone(s)) {
            let mut guard = storage.lock();
            *guard = Some((provider.into(), model.into()));
        }
    }

    pub fn clear() {
        if let Ok(storage) = MODEL_SWITCH_REQUEST.try_with(|s| Arc::clone(s)) {
            let mut guard = storage.lock();
            *guard = None;
        }
    }
}

pub fn get_model_switch_state() -> ModelSwitchCallback {
    ModelSwitchGuard::get_callback()
}

pub fn clear_model_switch_request() {
    ModelSwitchGuard::clear();
}
