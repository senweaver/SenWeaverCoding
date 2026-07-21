// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::background::registry::{self, BackgroundShellSignal, BgStream};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

const FOREGROUND_STREAM_CAP: usize = 1_048_576;

fn utf8_floor_boundary(bytes: &[u8], target: usize) -> usize {
    if target >= bytes.len() {
        return bytes.len();
    }
    let mut idx = target;
    while idx > 0 && (bytes[idx] & 0xC0) == 0x80 {
        idx -= 1;
    }
    idx
}

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

struct StreamReaderHandle {
    rx: tokio::sync::oneshot::Receiver<String>,
    buf: Arc<Mutex<Vec<u8>>>,
}

fn spawn_stream_reader<R>(
    pipe: Option<R>,
    mirror_id: String,
    stream: BgStream,
    session_id: Option<String>,
    label: &'static str,
) -> StreamReaderHandle
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    // Shared with the drain step so that, when a lingering grandchild keeps the
    // pipe open past EOF, the drain timeout can still snapshot everything read so
    // far instead of handing the model an empty string.
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let reader_buf = buf.clone();
    crate::runtime::spawn_supervised(label, async move {
        let mut capped = false;
        if let Some(pipe) = pipe {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(pipe);
            let mut line: Vec<u8> = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if !capped {
                            let mut raw_all = reader_buf.lock();
                            raw_all.extend_from_slice(&line);
                            if raw_all.len() > FOREGROUND_STREAM_CAP {
                                let boundary = utf8_floor_boundary(&raw_all, FOREGROUND_STREAM_CAP);
                                raw_all.truncate(boundary);
                                raw_all.extend_from_slice(b"\n... [output truncated at 1MB]");
                                capped = true;
                            }
                        }
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
        let decoded = {
            let raw_all = reader_buf.lock();
            crate::util::decode_subprocess_bytes(&raw_all)
        };
        let _ = tx.send(decoded);
    });
    StreamReaderHandle { rx, buf }
}

fn drain_stream(handle: StreamReaderHandle) -> impl std::future::Future<Output = String> {
    async move {
        match tokio::time::timeout(Duration::from_millis(500), handle.rx).await {
            Ok(Ok(text)) => text,
            // The reader task never signalled EOF within the grace window (a
            // grandchild is still holding the pipe). Return whatever it has
            // buffered so far rather than an empty string.
            _ => {
                let raw_all = handle.buf.lock();
                crate::util::decode_subprocess_bytes(&raw_all)
            }
        }
    }
}

pub(crate) async fn run_foreground_streamed(
    mut child: tokio::process::Child,
    mirror_id: &str,
    mirror_session_id: Option<&str>,
    mirror_started: std::time::Instant,
    timeout_duration: Duration,
) -> ForegroundOutcome {
    let stdout_handle = spawn_stream_reader(
        child.stdout.take(),
        mirror_id.to_string(),
        BgStream::Stdout,
        mirror_session_id.map(str::to_string),
        "tools.shell.stdout",
    );
    let stderr_handle = spawn_stream_reader(
        child.stderr.take(),
        mirror_id.to_string(),
        BgStream::Stderr,
        mirror_session_id.map(str::to_string),
        "tools.shell.stderr",
    );

    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    let (foreground_token, _kill_tx_keepalive) = if let Some(sid) = mirror_session_id {
        let connection_id = crate::session::current_connection_id();
        (
            Some(registry::register_foreground(
                sid.to_string(),
                connection_id,
                kill_tx,
            )),
            None,
        )
    } else {
        (None, Some(kill_tx))
    };

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
                crate::util::kill_child_process_tree(&mut child).await;
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
                crate::util::kill_child_process_tree(&mut child).await;
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
                break WaitOutcome::Cancelled;
            }
            _ = heartbeat.tick() => {
                registry::publish(BackgroundShellSignal::Heartbeat {
                    id: mirror_id.to_string(),
                    elapsed_secs: mirror_started.elapsed().as_secs(),
                    session_id: mirror_session_id.map(str::to_string),
                });
            }
        }
    };

    if let (Some(sid), Some(token)) = (mirror_session_id, foreground_token) {
        registry::unregister_foreground(sid, token);
    }

    let drained_stdout = drain_stream(stdout_handle).await;
    let drained_stderr = drain_stream(stderr_handle).await;

    match wait_outcome {
        WaitOutcome::Exited(status) => {
            ForegroundOutcome::Exited(status, drained_stdout, drained_stderr)
        }
        WaitOutcome::WaitError(e) => ForegroundOutcome::WaitError(e),
        WaitOutcome::Timeout => ForegroundOutcome::Timeout(drained_stdout, drained_stderr),
        WaitOutcome::Cancelled => ForegroundOutcome::Cancelled(drained_stdout, drained_stderr),
    }
}
