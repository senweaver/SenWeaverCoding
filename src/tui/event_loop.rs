// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Event-driven TUI input plumbing.
//!
//! Replaces the -era `crossterm::event::poll(100ms)` busy-loop
//! with a `tokio::task::spawn_blocking` worker that funnels
//! [`crossterm::event::Event`]s into a `tokio::sync::mpsc`
//! unbounded channel, so the main `run_tui_inner` task can
//! [`tokio::select!`] on input, agent deltas, and a 16 ms redraw tick
//! concurrently.
//!
//! # Why `spawn_blocking` and not `crossterm::event::EventStream`?
//!
//! `EventStream` (via `crossterm::event` + tokio bindings) has a known
//! regression on Windows ConPTY hosts where events stop arriving after
//! the first resize; see the `risk 4 — TUI event-loop` mitigation in
//! the master plan.  The blocking worker pattern is portable across
//! Linux/macOS/Windows and requires no new optional features.
//!
//! # Shutdown
//!
//! The worker polls in 50 ms ticks so it can notice the shared
//! [`std::sync::atomic::AtomicBool`] shutdown flag shortly after the
//! main loop exits.  Worst-case clean-up latency is ~50 ms; when
//! `crossterm::event::read` is parked on a final key, the event is
//! still delivered before the worker exits.

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
        loop {
            if shutdown_thread.load(Ordering::Acquire) {
                break;
            }
            match event::poll(poll_budget) {
                Ok(true) => match event::read() {
                    Ok(ev) => {

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
                Ok(false) => continue,
                Err(e) => {
                    tracing::warn!(
                        target: "tui.event_loop",
                        "crossterm::event::poll failed: {e}"
                    );

                    std::thread::sleep(Duration::from_millis(100));
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
