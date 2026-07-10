// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static GATEWAY_SHUTDOWN: parking_lot::RwLock<Option<tokio::sync::watch::Sender<bool>>> =
    parking_lot::RwLock::new(None);
pub(crate) static GATEWAY_RUNNING: AtomicBool = AtomicBool::new(false);
pub(crate) static GATEWAY_FULLY_STOPPED: AtomicBool = AtomicBool::new(false);
static GATEWAY_SHUTDOWN_PENDING: AtomicBool = AtomicBool::new(false);

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

pub(crate) fn install_shutdown_sender(tx: tokio::sync::watch::Sender<bool>) {
    // A restart may have requested shutdown before this instance finished wiring
    // its channel. Honor that pending request immediately so the just-started
    // instance stops instead of leaking alongside its replacement.
    if GATEWAY_SHUTDOWN_PENDING.swap(false, Ordering::SeqCst) {
        let _ = tx.send(true);
    }
    *GATEWAY_SHUTDOWN.write() = Some(tx);
}

pub(crate) fn clear_shutdown_sender() {
    *GATEWAY_SHUTDOWN.write() = None;
}

pub fn request_shutdown() -> bool {
    let guard = GATEWAY_SHUTDOWN.read();
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(true);
        true
    } else {
        false
    }
}

pub fn request_embedded_shutdown() -> bool {
    if request_shutdown() {
        return true;
    }
    // The gateway is starting but has not wired its shutdown channel yet. Latch
    // the request so `install_shutdown_sender` fires it as soon as the channel
    // exists, preventing a double-gateway leak on restart.
    if is_running() {
        GATEWAY_SHUTDOWN_PENDING.store(true, Ordering::SeqCst);
    }
    false
}

pub async fn wait_embedded_stopped(timeout: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !is_running() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

pub fn is_shutdown_requested() -> bool {
    GATEWAY_SHUTDOWN
        .read()
        .as_ref()
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
        clear_shutdown_sender();
        GATEWAY_RUNNING.store(false, Ordering::SeqCst);
        GATEWAY_FULLY_STOPPED.store(true, Ordering::SeqCst);
    }
}
