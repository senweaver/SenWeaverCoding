// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
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
        session_id: Option<String>,
    },

    Chunk {
        id: String,
        stream: BgStream,
        line: String,
        session_id: Option<String>,
    },

    Heartbeat {
        id: String,
        elapsed_secs: u64,
        session_id: Option<String>,
    },

    Exited {
        id: String,
        elapsed_secs: u64,
        exit_code: Option<i32>,
        session_id: Option<String>,
    },
}

struct ChildHandle {
    command: String,

    kill_tx: Option<oneshot::Sender<()>>,
}

struct ForegroundEntry {
    token: u64,
    connection_id: Option<String>,
    kill_tx: oneshot::Sender<()>,
}

struct RegistryInner {
    children: Mutex<HashMap<String, ChildHandle>>,
    foreground: Mutex<HashMap<String, Vec<ForegroundEntry>>>,
    foreground_seq: AtomicU64,
    tx: broadcast::Sender<BackgroundShellSignal>,
}

static REGISTRY: OnceLock<RegistryInner> = OnceLock::new();

fn registry() -> &'static RegistryInner {
    REGISTRY.get_or_init(|| {
        let (tx, _) = broadcast::channel(256);
        RegistryInner {
            children: Mutex::new(HashMap::new()),
            foreground: Mutex::new(HashMap::new()),
            foreground_seq: AtomicU64::new(0),
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

pub(crate) fn register(
    id: String,
    command: String,
    kill_tx: oneshot::Sender<()>,
    session_id: Option<String>,
) {
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
    publish(BackgroundShellSignal::Spawned {
        id,
        command,
        session_id,
    });
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

pub(crate) fn register_foreground(
    session_id: String,
    connection_id: Option<String>,
    kill_tx: oneshot::Sender<()>,
) -> u64 {
    let reg = registry();
    let token = reg.foreground_seq.fetch_add(1, Ordering::Relaxed);
    let mut guard = reg.foreground.lock().unwrap_or_else(|e| e.into_inner());
    guard.entry(session_id).or_default().push(ForegroundEntry {
        token,
        connection_id,
        kill_tx,
    });
    token
}

pub(crate) fn unregister_foreground(session_id: &str, token: u64) {
    let mut guard = registry()
        .foreground
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(entries) = guard.get_mut(session_id) {
        entries.retain(|entry| entry.token != token);
        if entries.is_empty() {
            guard.remove(session_id);
        }
    }
}

pub fn kill_foreground(session_id: &str, connection_id: Option<&str>) -> bool {
    let mut guard = registry()
        .foreground
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(list) = guard.get_mut(session_id) else {
        return false;
    };

    let mut killed = false;
    match connection_id {
        Some(conn) => {
            let mut remaining = Vec::with_capacity(list.len());
            for entry in list.drain(..) {
                let matches = entry
                    .connection_id
                    .as_deref()
                    .map_or(false, |c| c == conn);
                if matches {
                    let _ = entry.kill_tx.send(());
                    killed = true;
                } else {
                    remaining.push(entry);
                }
            }
            *list = remaining;
        }
        None => {
            for entry in list.drain(..) {
                let _ = entry.kill_tx.send(());
                killed = true;
            }
        }
    }

    if list.is_empty() {
        guard.remove(session_id);
    }
    killed
}
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
