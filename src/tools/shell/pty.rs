// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#![cfg(feature = "pty")]

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PTY_OUTPUT_CAP_BYTES: usize = 4 * 1024 * 1024;
const PTY_ROWS: u16 = 40;
const PTY_COLS: u16 = 160;

pub struct PtyRunOutcome {
    pub exit_code: Option<i32>,
    pub output: String,
    pub timed_out: bool,
    pub truncated: bool,
}

pub async fn run_command_in_pty(
    command: String,
    workspace_dir: PathBuf,
    env: Vec<(String, String)>,
    timeout: Duration,
) -> anyhow::Result<PtyRunOutcome> {
    tokio::task::spawn_blocking(move || run_blocking(&command, &workspace_dir, env, timeout))
        .await
        .map_err(|e| anyhow::anyhow!("pty task join error: {e}"))?
}

fn run_blocking(
    command: &str,
    workspace_dir: &std::path::Path,
    env: Vec<(String, String)>,
    timeout: Duration,
) -> anyhow::Result<PtyRunOutcome> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow::anyhow!("openpty failed: {e}"))?;

    let mut builder = if cfg!(windows) {
        let mut b = CommandBuilder::new("cmd.exe");
        b.args(["/C", &format!("chcp 65001 >nul & {command}")]);
        b
    } else {
        let mut b = CommandBuilder::new("/bin/sh");
        b.args(["-lc", command]);
        b
    };
    builder.cwd(workspace_dir);
    builder.env_clear();
    for (k, v) in env {
        builder.env(k, v);
    }

    let mut child = pair
        .slave
        .spawn_command(builder)
        .map_err(|e| anyhow::anyhow!("pty spawn failed: {e}"))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow::anyhow!("pty reader unavailable: {e}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let mut collected: Vec<u8> = Vec::new();
    let mut timed_out = false;
    let mut truncated = false;
    let mut exit_code: Option<i32> = None;

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| anyhow::anyhow!("pty wait failed: {e}"))?
        {
            exit_code = Some(status.exit_code() as i32);
            let drain_deadline = Instant::now() + Duration::from_millis(400);
            while Instant::now() < drain_deadline {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(chunk) => {
                        if collected.len() < PTY_OUTPUT_CAP_BYTES {
                            collected.extend(chunk);
                        } else {
                            truncated = true;
                        }
                    }
                    Err(_) => break,
                }
            }
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                if collected.len() < PTY_OUTPUT_CAP_BYTES {
                    collected.extend(chunk);
                } else {
                    truncated = true;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                match child.wait() {
                    Ok(status) => exit_code = Some(status.exit_code() as i32),
                    Err(_) => {}
                }
                break;
            }
        }
    }
    drop(pair.master);

    if collected.len() > PTY_OUTPUT_CAP_BYTES {
        collected.truncate(PTY_OUTPUT_CAP_BYTES);
        truncated = true;
    }
    let decoded = crate::util::decode_subprocess_bytes(&collected);
    let output = crate::token_saver::pipeline::strip_ansi_only(&decoded);

    Ok(PtyRunOutcome {
        exit_code,
        output,
        timed_out,
        truncated,
    })
}
