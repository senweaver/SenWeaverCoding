// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) static GATEWAY_SHUTDOWN_SIGNAL: OnceLock<tokio::sync::watch::Sender<bool>> =
    OnceLock::new();
pub(crate) static GATEWAY_RUNNING: AtomicBool = AtomicBool::new(false);
pub(crate) static GATEWAY_FULLY_STOPPED: AtomicBool = AtomicBool::new(false);

static GATEWAY_STARTUP_WARNINGS: OnceLock<parking_lot::RwLock<Vec<StartupWarning>>> =
    OnceLock::new();

#[derive(Debug, Clone, serde::Serialize)]
pub struct StartupWarning {
    pub subtype: String,
    pub message: String,
}

fn startup_warnings_store() -> &'static parking_lot::RwLock<Vec<StartupWarning>> {
    GATEWAY_STARTUP_WARNINGS.get_or_init(|| parking_lot::RwLock::new(Vec::new()))
}

pub fn push_startup_warning(subtype: impl Into<String>, message: impl Into<String>) {
    let warning = StartupWarning {
        subtype: subtype.into(),
        message: message.into(),
    };
    startup_warnings_store().write().push(warning);
}

pub fn snapshot_startup_warnings() -> Vec<StartupWarning> {
    startup_warnings_store().read().clone()
}

pub fn request_shutdown() -> bool {
    if let Some(tx) = GATEWAY_SHUTDOWN_SIGNAL.get() {
        let _ = tx.send(true);
        true
    } else {
        false
    }
}

pub fn is_shutdown_requested() -> bool {
    GATEWAY_SHUTDOWN_SIGNAL
        .get()
        .map(|tx| *tx.borrow())
        .unwrap_or(false)
}

pub fn is_running() -> bool {
    GATEWAY_RUNNING.load(Ordering::SeqCst)
}

pub fn is_fully_stopped() -> bool {
    GATEWAY_FULLY_STOPPED.load(Ordering::SeqCst)
}

pub(crate) struct GatewayRunningGuard;

impl GatewayRunningGuard {
    pub(crate) fn install() -> Self {
        GATEWAY_FULLY_STOPPED.store(false, Ordering::SeqCst);
        GATEWAY_RUNNING.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for GatewayRunningGuard {
    fn drop(&mut self) {
        GATEWAY_RUNNING.store(false, Ordering::SeqCst);
        GATEWAY_FULLY_STOPPED.store(true, Ordering::SeqCst);
    }
}
