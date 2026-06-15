// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    str,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(not(target_os = "windows"))]
use std::process::Stdio;

use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

const TERMINAL_OUTPUT_CHANNEL_CAPACITY: usize = 8192;
const TERMINAL_WRITE_CHANNEL_CAPACITY: usize = 4096;

#[derive(Default)]
pub(crate) struct TerminalState {
    pub(crate) next_id: AtomicU32,
    pub(crate) sessions: Mutex<HashMap<u32, TerminalSession>>,
}

pub(crate) struct TerminalSession {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    write_tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

#[derive(Serialize, Clone)]
pub(crate) struct TerminalSpawnResult {
    session_id: u32,
    shell: String,
    cwd: String,
}

#[derive(Serialize, Clone)]
struct TerminalOutputPayload {
    session_id: u32,
    data: String,
}

#[derive(Serialize, Clone)]
struct TerminalExitPayload {
    session_id: u32,
    code: u32,
}

struct SpawnedTerminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn std::io::Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    child: Box<dyn Child + Send + Sync>,
    reader: Box<dyn std::io::Read + Send>,
    shell: String,
    cwd: PathBuf,
}

fn spawn_terminal_blocking(
    cols: u16,
    rows: u16,
    cwd: Option<String>,
) -> Result<SpawnedTerminal, String> {
    let cwd_path = resolve_terminal_cwd(cwd)?;
    let shell = default_shell();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(8),
            cols: cols.max(20),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("open terminal pty: {err}"))?;

    let mut cmd = CommandBuilder::new(&shell);
    cmd.cwd(cwd_path.as_os_str());
    for (key, value) in terminal_environment(&shell, &cwd_path) {
        cmd.env(key, value);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| format!("spawn terminal shell: {err}"))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("clone terminal reader: {err}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("open terminal writer: {err}"))?;
    let killer = child.clone_killer();

    Ok(SpawnedTerminal {
        master: pair.master,
        writer,
        killer,
        child,
        reader,
        shell,
        cwd: cwd_path,
    })
}

#[tauri::command]
pub(crate) async fn terminal_spawn(
    app: AppHandle,
    state: State<'_, TerminalState>,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
) -> Result<TerminalSpawnResult, String> {
    let spawned =
        tauri::async_runtime::spawn_blocking(move || spawn_terminal_blocking(cols, rows, cwd))
            .await
            .map_err(|err| format!("terminal spawn task failed: {err}"))??;
    let SpawnedTerminal {
        master,
        writer,
        killer,
        mut child,
        mut reader,
        shell,
        cwd: cwd_path,
    } = spawned;

    let session_id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;

    let (write_tx, write_rx) =
        std::sync::mpsc::sync_channel::<Vec<u8>>(TERMINAL_WRITE_CHANNEL_CAPACITY);
    thread::spawn(move || {
        let mut writer = writer;
        while let Ok(bytes) = write_rx.recv() {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            if writer.flush().is_err() {
                break;
            }
        }
    });

    {
        let mut sessions = state
            .sessions
            .lock()
            .map_err(|_| "terminal state is unavailable".to_string())?;
        sessions.insert(
            session_id,
            TerminalSession {
                master: Arc::new(Mutex::new(master)),
                write_tx,
                killer: Mutex::new(killer),
            },
        );
    }

    let output_app = app.clone();
    let (chunk_tx, chunk_rx) =
        std::sync::mpsc::sync_channel::<String>(TERMINAL_OUTPUT_CHANNEL_CAPACITY);
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        let mut pending_utf8 = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let data = decode_terminal_output(&mut pending_utf8, &buffer[..n]);
                    if data.is_empty() {
                        continue;
                    }
                    if chunk_tx.send(data).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let _ = chunk_tx.send(format!("\r\n[terminal read error: {err}]\r\n"));
                    break;
                }
            }
        }
        if !pending_utf8.is_empty() {
            let _ = chunk_tx.send(String::from_utf8_lossy(&pending_utf8).to_string());
        }
    });

    thread::spawn(move || {
        const COALESCE_WINDOW: Duration = Duration::from_millis(8);
        const MAX_PENDING_BYTES: usize = 262_144;
        static OUTPUT_EMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
        while let Ok(first) = chunk_rx.recv() {
            let mut pending = first;
            let deadline = Instant::now() + COALESCE_WINDOW;
            while pending.len() < MAX_PENDING_BYTES {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                match chunk_rx.recv_timeout(deadline - now) {
                    Ok(more) => pending.push_str(&more),
                    Err(_) => break,
                }
            }
            if let Err(err) = output_app.emit(
                "terminal-output",
                TerminalOutputPayload {
                    session_id,
                    data: pending,
                },
            ) {
                crate::warn_emit_failure(&OUTPUT_EMIT_FAILURES, "terminal-output", &err);
            }
        }
    });

    let exit_app = app.clone();
    thread::spawn(move || {
        static EXIT_EMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
        static WAIT_ERROR_EMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
        let status = child.wait();
        if let Some(state) = exit_app.try_state::<TerminalState>() {
            if let Ok(mut sessions) = state.sessions.lock() {
                sessions.remove(&session_id);
            }
        }
        match status {
            Ok(status) => {
                if let Err(err) = exit_app.emit(
                    "terminal-exit",
                    TerminalExitPayload {
                        session_id,
                        code: status.exit_code(),
                    },
                ) {
                    crate::warn_emit_failure(&EXIT_EMIT_FAILURES, "terminal-exit", &err);
                }
            }
            Err(err) => {
                if let Err(emit_err) = exit_app.emit(
                    "terminal-output",
                    TerminalOutputPayload {
                        session_id,
                        data: format!("\r\n[terminal wait error: {err}]\r\n"),
                    },
                ) {
                    crate::warn_emit_failure(
                        &WAIT_ERROR_EMIT_FAILURES,
                        "terminal-output (wait error)",
                        &emit_err,
                    );
                }
            }
        }
    });

    Ok(TerminalSpawnResult {
        session_id,
        shell,
        cwd: cwd_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub(crate) async fn terminal_write(
    state: State<'_, TerminalState>,
    session_id: u32,
    data: String,
) -> Result<(), String> {
    let sessions = state
        .sessions
        .lock()
        .map_err(|_| "terminal state is unavailable".to_string())?;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| "terminal session is not running".to_string())?;
    session
        .write_tx
        .try_send(data.into_bytes())
        .map_err(|err| match err {
            std::sync::mpsc::TrySendError::Full(_) => "terminal write buffer full".to_string(),
            std::sync::mpsc::TrySendError::Disconnected(_) => {
                "terminal session is not running".to_string()
            }
        })
}

#[tauri::command]
pub(crate) async fn terminal_resize(
    state: State<'_, TerminalState>,
    session_id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let master = {
        let sessions = state
            .sessions
            .lock()
            .map_err(|_| "terminal state is unavailable".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| "terminal session is not running".to_string())?;
        session.master.clone()
    };
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let master = master
            .lock()
            .map_err(|_| "terminal master is unavailable".to_string())?;
        master
            .resize(PtySize {
                rows: rows.max(8),
                cols: cols.max(20),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| format!("resize terminal: {err}"))?;
        Ok(())
    })
    .await
    .map_err(|err| format!("terminal resize task failed: {err}"))?
}

#[tauri::command]
pub(crate) fn terminal_kill(
    state: State<'_, TerminalState>,
    session_id: u32,
) -> Result<(), String> {
    let session = {
        let mut sessions = state
            .sessions
            .lock()
            .map_err(|_| "terminal state is unavailable".to_string())?;
        sessions.remove(&session_id)
    };
    if let Some(session) = session {
        let mut killer = session
            .killer
            .lock()
            .map_err(|_| "terminal killer is unavailable".to_string())?;
        killer
            .kill()
            .map_err(|err| format!("kill terminal shell: {err}"))?;
    }
    Ok(())
}

pub(crate) fn shutdown_all(app: &AppHandle) {
    let Some(state) = app.try_state::<TerminalState>() else {
        return;
    };
    let mut sessions = match state.sessions.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    for (_, session) in sessions.drain() {
        if let Ok(mut killer) = session.killer.lock() {
            let _ = killer.kill();
        }
    }
}

fn decode_terminal_output(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    pending.extend_from_slice(chunk);
    let mut output = String::new();

    loop {
        match str::from_utf8(pending) {
            Ok(text) => {
                output.push_str(text);
                pending.clear();
                break;
            }
            Err(err) => {
                let valid_up_to = err.valid_up_to();
                if valid_up_to > 0 {
                    output.push_str(&String::from_utf8_lossy(&pending[..valid_up_to]));
                    pending.drain(..valid_up_to);
                    continue;
                }

                match err.error_len() {
                    Some(error_len) => {
                        output.push('\u{fffd}');
                        pending.drain(..error_len);
                    }
                    None => break,
                }
            }
        }
    }

    output
}

fn terminal_environment(shell: &str, workspace: &std::path::Path) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.extend(login_shell_environment(shell));
    ensure_utf8_locale(&mut env);
    for (k, v) in senweavercoding::python_env::activation_env(workspace) {
        env.insert(k, v);
    }
    env.remove("PYTHONHOME");
    env
}

fn ensure_utf8_locale(env: &mut HashMap<String, String>) {
    let fallback = default_utf8_locale();
    for key in ["LANG", "LC_CTYPE", "LC_ALL"] {
        let needs_fallback = env
            .get(key)
            .map(|value| !is_utf8_locale(value))
            .unwrap_or(true);
        if needs_fallback {
            env.insert(key.to_string(), fallback.to_string());
        }
    }
}

fn is_utf8_locale(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "");
    normalized.contains("utf8")
}

fn default_utf8_locale() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "en_US.UTF-8"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "C.UTF-8"
    }
    #[cfg(not(unix))]
    {
        "C.UTF-8"
    }
}

#[cfg(not(target_os = "windows"))]
fn login_shell_environment(shell: &str) -> HashMap<String, String> {
    let Ok(mut child) = senweavercoding::util::hidden_sync_command(shell)
        .args(["-l", "-c", "env -0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return HashMap::new();
    };

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return HashMap::new();
                }
                let mut stdout = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_end(&mut stdout);
                }
                return parse_env_block(&stdout);
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return HashMap::new();
            }
            Err(_) => return HashMap::new(),
        }
    }
}

#[cfg(target_os = "windows")]
fn login_shell_environment(_shell: &str) -> HashMap<String, String> {
    HashMap::new()
}

#[cfg(not(target_os = "windows"))]
fn parse_env_block(bytes: &[u8]) -> HashMap<String, String> {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            if entry.is_empty() {
                return None;
            }
            let equals = entry.iter().position(|byte| *byte == b'=')?;
            if equals == 0 {
                return None;
            }
            let key = String::from_utf8_lossy(&entry[..equals]).to_string();
            let value = String::from_utf8_lossy(&entry[equals + 1..]).to_string();
            Some((key, value))
        })
        .collect()
}

fn resolve_terminal_cwd(cwd: Option<String>) -> Result<PathBuf, String> {
    let explicit = cwd.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    });

    let path = match explicit {
        Some(path) => path,
        None => match std::env::current_dir() {
            Ok(p) => p,
            Err(_) => match home_dir() {
                Some(h) => h,
                None => {
                    return Err("resolve terminal cwd: no current_dir and no home dir".to_string());
                }
            },
        },
    };

    if path.is_dir() {
        return Ok(path);
    }

    if path.is_file() {
        if let Some(parent) = path.parent() {
            if parent.is_dir() {
                return Ok(parent.to_path_buf());
            }
        }
    }

    if path.exists() {
        if let Some(parent) = path.parent() {
            if parent.is_dir() {
                return Ok(parent.to_path_buf());
            }
        }
    }

    Err(format!("terminal cwd does not exist: {}", path.display()))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn default_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| {
            if PathBuf::from("/bin/zsh").exists() {
                "/bin/zsh".to_string()
            } else {
                "/bin/bash".to_string()
            }
        })
    }
}
