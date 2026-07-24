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

    session_id: Option<String>,

    kill_tx: Option<oneshot::Sender<()>>,
}

struct ForegroundEntry {
    token: u64,
    connection_id: Option<String>,
    kill_tx: oneshot::Sender<()>,
}

const MAX_BUFFERED_BYTES_PER_SHELL: usize = 262_144;
const MAX_BUFFERED_LINES_PER_SHELL: usize = 2_000;
const MAX_RETAINED_EXITED_SHELLS: usize = 16;

pub struct BgOutputSnapshot {
    pub id: String,
    pub command: String,
    pub session_id: Option<String>,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub elapsed_secs: u64,
    pub buffered_lines: usize,
    pub dropped_lines: usize,
}

struct BgFinished {
    code: Option<i32>,
    at: std::time::Instant,
}

struct BgOutputBuf {
    command: String,
    session_id: Option<String>,
    lines: std::collections::VecDeque<(BgStream, String)>,
    bytes: usize,
    dropped_lines: usize,
    started_at: std::time::Instant,
    finished: Option<BgFinished>,
    detached: bool,
}

struct RegistryInner {
    children: Mutex<HashMap<String, ChildHandle>>,
    foreground: Mutex<HashMap<String, Vec<ForegroundEntry>>>,
    outputs: Mutex<HashMap<String, BgOutputBuf>>,
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
            outputs: Mutex::new(HashMap::new()),
            foreground_seq: AtomicU64::new(0),
            tx,
        }
    })
}

pub fn subscribe() -> broadcast::Receiver<BackgroundShellSignal> {
    registry().tx.subscribe()
}

fn record_signal_in_outputs(signal: &BackgroundShellSignal) {
    let mut guard = registry()
        .outputs
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match signal {
        BackgroundShellSignal::Spawned {
            id,
            command,
            session_id,
        } => {
            match guard.entry(id.clone()) {
                std::collections::hash_map::Entry::Occupied(mut existing) => {
                    let buf = existing.get_mut();
                    if buf.command.is_empty() && !command.is_empty() {
                        buf.command = command.clone();
                    }
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(BgOutputBuf {
                        command: command.clone(),
                        session_id: session_id.clone(),
                        lines: std::collections::VecDeque::new(),
                        bytes: 0,
                        dropped_lines: 0,
                        started_at: std::time::Instant::now(),
                        finished: None,
                        detached: false,
                    });
                }
            }
        }
        BackgroundShellSignal::Chunk {
            id, stream, line, ..
        } => {
            if let Some(buf) = guard.get_mut(id) {
                buf.bytes += line.len();
                buf.lines.push_back((*stream, line.clone()));
                while buf.lines.len() > MAX_BUFFERED_LINES_PER_SHELL
                    || buf.bytes > MAX_BUFFERED_BYTES_PER_SHELL
                {
                    if let Some((_, dropped)) = buf.lines.pop_front() {
                        buf.bytes = buf.bytes.saturating_sub(dropped.len());
                        buf.dropped_lines += 1;
                    } else {
                        break;
                    }
                }
            }
        }
        BackgroundShellSignal::Exited { id, exit_code, .. } => {
            if let Some(buf) = guard.get_mut(id) {
                buf.finished = Some(BgFinished {
                    code: *exit_code,
                    at: std::time::Instant::now(),
                });
            }
            let exited_count = guard.values().filter(|b| b.finished.is_some()).count();
            if exited_count > MAX_RETAINED_EXITED_SHELLS {
                let mut exited_ids: Vec<(String, std::time::Instant)> = guard
                    .iter()
                    .filter_map(|(k, b)| b.finished.as_ref().map(|f| (k.clone(), f.at)))
                    .collect();
                exited_ids.sort_by_key(|(_, t)| *t);
                for (old_id, _) in exited_ids
                    .into_iter()
                    .take(exited_count - MAX_RETAINED_EXITED_SHELLS)
                {
                    guard.remove(&old_id);
                }
            }
        }
        BackgroundShellSignal::Heartbeat { .. } => {}
    }
}

pub(crate) fn publish(signal: BackgroundShellSignal) {
    record_signal_in_outputs(&signal);
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
            session_id: session_id.clone(),
            kill_tx: Some(kill_tx),
        },
    );
    drop(guard);
    publish(BackgroundShellSignal::Spawned {
        id: id.clone(),
        command,
        session_id,
    });
    let mut outputs = registry()
        .outputs
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(buf) = outputs.get_mut(&id) {
        buf.detached = true;
    }
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
    let scope = crate::session::current_session_context()
        .map(|ctx| ctx.session_id)
        .filter(|s| !s.is_empty());
    let guard = registry()
        .children
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    guard
        .iter()
        .filter(|(_, v)| match (&scope, &v.session_id) {
            (Some(want), Some(have)) => want == have,
            (Some(_), None) => false,
            (None, Some(_)) => false,
            (None, None) => true,
        })
        .map(|(k, v)| (k.clone(), v.command.clone()))
        .collect()
}

fn session_scope_allows(scope: &Option<String>, entry_session: &Option<String>) -> bool {
    match (scope, entry_session) {
        (Some(want), Some(have)) => want == have,
        (Some(_), None) => false,
        (None, Some(_)) => false,
        (None, None) => true,
    }
}

fn snapshot_of(id: &str, buf: &BgOutputBuf) -> BgOutputSnapshot {
    BgOutputSnapshot {
        id: id.to_string(),
        command: buf.command.clone(),
        session_id: buf.session_id.clone(),
        running: buf.finished.is_none(),
        exit_code: buf.finished.as_ref().and_then(|f| f.code),
        elapsed_secs: buf
            .finished
            .as_ref()
            .map(|f| f.at)
            .unwrap_or_else(std::time::Instant::now)
            .duration_since(buf.started_at)
            .as_secs(),
        buffered_lines: buf.lines.len(),
        dropped_lines: buf.dropped_lines,
    }
}

pub fn status_snapshot() -> Vec<BgOutputSnapshot> {
    let scope = crate::session::current_session_context()
        .map(|ctx| ctx.session_id)
        .filter(|s| !s.is_empty());
    let tracked: std::collections::HashSet<String> = registry()
        .children
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    let guard = registry()
        .outputs
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<BgOutputSnapshot> = guard
        .iter()
        .filter(|(id, buf)| {
            id.starts_with("bg-") || buf.detached || tracked.contains(id.as_str())
        })
        .filter(|(_, buf)| session_scope_allows(&scope, &buf.session_id))
        .map(|(id, buf)| snapshot_of(id, buf))
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn logs_for(id: &str, tail_lines: usize) -> Option<(BgOutputSnapshot, String)> {
    let scope = crate::session::current_session_context()
        .map(|ctx| ctx.session_id)
        .filter(|s| !s.is_empty());
    let guard = registry()
        .outputs
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let buf = guard.get(id)?;
    if !session_scope_allows(&scope, &buf.session_id) {
        return None;
    }
    let take = tail_lines.clamp(1, MAX_BUFFERED_LINES_PER_SHELL);
    let start = buf.lines.len().saturating_sub(take);
    let mut text = String::new();
    for (stream, line) in buf.lines.iter().skip(start) {
        match stream {
            BgStream::Stdout => text.push_str(line),
            BgStream::Stderr => {
                text.push_str("[stderr] ");
                text.push_str(line);
            }
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    Some((snapshot_of(id, buf), text))
}
