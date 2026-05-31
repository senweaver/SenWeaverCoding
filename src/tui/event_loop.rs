// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event};
use tokio::sync::mpsc;

pub struct InputThreadHandle {
    pub rx: mpsc::UnboundedReceiver<Event>,
    shutdown: Arc<AtomicBool>,
}

impl InputThreadHandle {

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

impl Drop for InputThreadHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

pub fn spawn_input_thread() -> InputThreadHandle {
    let (tx, rx) = mpsc::unbounded_channel::<Event>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_thread = shutdown.clone();
    tokio::task::spawn_blocking(move || {

        let poll_budget = Duration::from_millis(50);
        let mut backoff_ms = 50u64;
        loop {
            if shutdown_thread.load(Ordering::Acquire) {
                break;
            }
            match event::poll(poll_budget) {
                Ok(true) => match event::read() {
                    Ok(ev) => {
                        backoff_ms = 50;
                        if tx.send(ev).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "tui.event_loop",
                            "crossterm::event::read failed: {e}"
                        );
                        break;
                    }
                },
                Ok(false) => {
                    backoff_ms = 50;
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        target: "tui.event_loop",
                        "crossterm::event::poll failed (retry in {backoff_ms}ms): {e}"
                    );

                    std::thread::sleep(Duration::from_millis(backoff_ms));
                    backoff_ms = (backoff_ms * 2).min(200);
                }
            }
        }
    });
    InputThreadHandle { rx, shutdown }
}

pub fn is_legacy_loop_enabled(cli_flag: bool) -> bool {
    if cli_flag {
        return true;
    }
    match std::env::var("TUI_LEGACY_LOOP") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}
