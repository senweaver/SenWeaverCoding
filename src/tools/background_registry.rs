// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Background shell registry + event broadcast.
//!
//! Powers the GUI's background-shell cards (図 3-5): when
//! [`ShellTool::execute`](super::shell::ShellTool) is called with
//! `background: true`, the spawned child is **registered** here so
//! the GUI / TUI bridge can:
//!
//! * subscribe to a global broadcast channel of
//!   [`BackgroundShellSignal`]s (live stdout/stderr lines, heart-
//!   beats, exit notifications) and translate them into
//!   `AgentEvent::BackgroundShell{,Chunk}`,
//! * issue `kill(id)` to terminate a running child via a
//!   `oneshot::Sender<()>`-based abort token (used by the GUI's
//!   `Stop` row in the input-area drop-up panel and by
//!   `UserInput::KillBackgroundShell`).
//!
//! The registry is intentionally process-wide (not per-Agent /
//! per-Turn) because background shells outlive turns by design:
//! a `cargo build` kicked off in turn N may still be running when
//! the user types a new prompt in turn N+1 and the GUI must keep
//! showing its rolling tail.
//!
//! The broadcast channel has capacity 256; slow consumers see
//! lagged events but the registry never blocks the producer.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tokio::sync::{broadcast, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub enum BackgroundShellSignal {

    Spawned {
        id: String,
        command: String,
    },

    Chunk {
        id: String,
        stream: BgStream,
        line: String,
    },

    Heartbeat {
        id: String,
        elapsed_secs: u64,
    },

    Exited {
        id: String,
        elapsed_secs: u64,
        exit_code: Option<i32>,
    },
}

struct ChildHandle {
    command: String,

    kill_tx: Option<oneshot::Sender<()>>,
}

struct RegistryInner {
    children: Mutex<HashMap<String, ChildHandle>>,
    tx: broadcast::Sender<BackgroundShellSignal>,
}

static REGISTRY: OnceLock<RegistryInner> = OnceLock::new();

fn registry() -> &'static RegistryInner {
    REGISTRY.get_or_init(|| {
        let (tx, _) = broadcast::channel(256);
        RegistryInner {
            children: Mutex::new(HashMap::new()),
            tx,
        }
    })
}

pub fn subscribe() -> broadcast::Receiver<BackgroundShellSignal> {
    registry().tx.subscribe()
}

pub(crate) fn publish(signal: BackgroundShellSignal) {
    let _ = registry().tx.send(signal);
}

pub(crate) fn register(id: String, command: String, kill_tx: oneshot::Sender<()>) {
    let mut guard = registry()
        .children
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    guard.insert(
        id.clone(),
        ChildHandle {
            command: command.clone(),
            kill_tx: Some(kill_tx),
        },
    );
    drop(guard);
    publish(BackgroundShellSignal::Spawned { id, command });
}

pub(crate) fn unregister(id: &str) {
    let mut guard = registry()
        .children
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    guard.remove(id);
}

pub fn kill(id: &str) -> bool {
    let mut guard = registry()
        .children
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(handle) = guard.get_mut(id) {
        if let Some(tx) = handle.kill_tx.take() {
            let _ = tx.send(());
            return true;
        }
    }
    false
}

#[allow(dead_code)]
pub fn snapshot() -> Vec<(String, String)> {
    let guard = registry()
        .children
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    guard
        .iter()
        .map(|(k, v)| (k.clone(), v.command.clone()))
        .collect()
}
