// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::background_registry::{self, BackgroundShellSignal, BgStream};
use std::time::Duration;

pub(crate) enum ForegroundOutcome {
    Exited(std::process::ExitStatus, String, String),
    Timeout(String, String),
    Cancelled(String, String),
    WaitError(std::io::Error),
}

pub(crate) const CANCELLED_BANNER: &str = "The user manually cancelled this command. \
     Do not retry this exact command unless it is essential; \
     prefer to continue with the next step or work around it.";

pub(crate) fn build_cancelled_output(stdout: &str, stderr: &str) -> String {
    let mut detail = String::new();
    if !stdout.is_empty() {
        detail.push_str(stdout);
        if !detail.ends_with('\n') {
            detail.push('\n');
        }
    }
    if !stderr.is_empty() {
        detail.push_str("--- stderr ---\n");
        detail.push_str(stderr);
        if !detail.ends_with('\n') {
            detail.push('\n');
        }
    }
    if detail.is_empty() {
        format!("[command cancelled by user]\n{CANCELLED_BANNER}")
    } else {
        format!("{detail}[command cancelled by user]\n{CANCELLED_BANNER}")
    }
}

fn spawn_stream_reader<R>(
    pipe: Option<R>,
    mirror_id: String,
    stream: BgStream,
    session_id: Option<String>,
    label: &'static str,
) -> tokio::sync::oneshot::Receiver<String>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    crate::runtime::spawn_supervised(label, async move {
        let mut raw_all: Vec<u8> = Vec::new();
        if let Some(pipe) = pipe {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(pipe);
            let mut line: Vec<u8> = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        raw_all.extend_from_slice(&line);
                        let decoded = crate::util::decode_subprocess_bytes(&line);
                        super::core::emit_mirror_chunks(
                            &mirror_id,
                            &decoded,
                            stream,
                            session_id.as_deref(),
                        );
                    }
                    Err(_) => break,
                }
            }
        }
        let _ = tx.send(crate::util::decode_subprocess_bytes(&raw_all));
    });
    rx
}

pub(crate) async fn run_foreground_streamed(
    mut child: tokio::process::Child,
    mirror_id: &str,
    mirror_session_id: Option<&str>,
    mirror_started: std::time::Instant,
    timeout_duration: Duration,
) -> ForegroundOutcome {
    let stdout_rx = spawn_stream_reader(
        child.stdout.take(),
        mirror_id.to_string(),
        BgStream::Stdout,
        mirror_session_id.map(str::to_string),
        "tools.shell.stdout",
    );
    let stderr_rx = spawn_stream_reader(
        child.stderr.take(),
        mirror_id.to_string(),
        BgStream::Stderr,
        mirror_session_id.map(str::to_string),
        "tools.shell.stderr",
    );

    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    if let Some(sid) = mirror_session_id {
        background_registry::register_foreground(sid.to_string(), kill_tx);
    }

    enum WaitOutcome {
        Exited(std::process::ExitStatus),
        WaitError(std::io::Error),
        Timeout,
        Cancelled,
    }

    let sleep = tokio::time::sleep(timeout_duration);
    tokio::pin!(sleep);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(2));
    heartbeat.tick().await;

    let wait_outcome = loop {
        tokio::select! {
            waited = child.wait() => {
                break match waited {
                    Ok(status) => WaitOutcome::Exited(status),
                    Err(e) => WaitOutcome::WaitError(e),
                };
            }
            _ = &mut sleep => {
                let _ = child.start_kill();
                if tokio::time::timeout(Duration::from_secs(3), child.wait())
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        mirror_id,
                        "foreground child did not exit within 3s after timeout kill; treating as orphan"
                    );
                }
                break WaitOutcome::Timeout;
            }
            _ = &mut kill_rx => {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
                break WaitOutcome::Cancelled;
            }
            _ = heartbeat.tick() => {
                background_registry::publish(BackgroundShellSignal::Heartbeat {
                    id: mirror_id.to_string(),
                    elapsed_secs: mirror_started.elapsed().as_secs(),
                    session_id: mirror_session_id.map(str::to_string),
                });
            }
        }
    };

    if let Some(sid) = mirror_session_id {
        background_registry::unregister_foreground(sid);
    }

    let drained_stdout = tokio::time::timeout(Duration::from_millis(500), stdout_rx)
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    let drained_stderr = tokio::time::timeout(Duration::from_millis(500), stderr_rx)
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();

    match wait_outcome {
        WaitOutcome::Exited(status) => {
            ForegroundOutcome::Exited(status, drained_stdout, drained_stderr)
        }
        WaitOutcome::WaitError(e) => ForegroundOutcome::WaitError(e),
        WaitOutcome::Timeout => ForegroundOutcome::Timeout(drained_stdout, drained_stderr),
        WaitOutcome::Cancelled => ForegroundOutcome::Cancelled(drained_stdout, drained_stderr),
    }
}
