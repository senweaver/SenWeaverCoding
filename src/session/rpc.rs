// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::observability::session_write_mode_metrics;
use crate::session::state::{RemoteDelta, SessionActor, SessionDelta};

#[async_trait::async_trait]
pub trait SessionRpcTransport: Send + Sync + 'static {

    async fn send(&self, delta: &SessionDelta) -> std::io::Result<()>;

    async fn recv(&self) -> std::io::Result<SessionDelta>;
}

#[cfg(unix)]
pub struct UdsTransport {
    path: PathBuf,
}

#[cfg(unix)]
impl UdsTransport {

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub async fn connect(&self) -> std::io::Result<tokio::net::UnixStream> {
        tokio::net::UnixStream::connect(&self.path).await
    }

    pub async fn listen(&self) -> std::io::Result<tokio::net::UnixListener> {
        tokio::net::UnixListener::bind(&self.path)
    }
}

#[cfg(windows)]
pub struct NamedPipeTransport {
    pipe_name: String,
}

#[cfg(windows)]
impl NamedPipeTransport {

    pub fn new(session_id: &str) -> Self {
        Self {
            pipe_name: format!(r"\\.\pipe\sen_session_{}", session_id),
        }
    }

    pub fn server(
        &self,
    ) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .create(&self.pipe_name)
    }
}

pub async fn spawn_rpc_listener(
    actor: Arc<SessionActor>,
    socket_path: PathBuf,
) -> tokio::task::JoinHandle<()> {
    crate::runtime::spawn_supervised("session.rpc.listener", async move {
        #[cfg(unix)]
        {

            let _ = std::fs::remove_file(&socket_path);

            let transport = UdsTransport::new(&socket_path);
            match transport.listen().await {
                Ok(listener) => {
                    tracing::debug!(
                        target: "session.rpc",
                        path = %socket_path.display(),
                        "UDS listener ready"
                    );
                    loop {
                        match listener.accept().await {
                            Ok((mut stream, _peer)) => {
                                let actor = actor.clone();
                                crate::runtime::spawn_supervised("session.rpc.conn", async move {
                                    let mut buf = Vec::new();
                                    if stream.read_to_end(&mut buf).await.is_ok() {
                                        match serde_json::from_slice::<RemoteDelta>(&buf) {
                                            Ok(remote) => {
                                                actor.apply_remote(remote);
                                            }
                                            Err(parse_err) => {

                                                if let Ok(delta) =
                                                    serde_json::from_slice::<SessionDelta>(&buf)
                                                {
                                                    let remote = RemoteDelta {
                                                        source_session_id: String::new(),
                                                        last_seen_seq: delta
                                                            .seq
                                                            .saturating_sub(1),
                                                        delta,
                                                    };
                                                    actor.apply_remote(remote);
                                                } else {
                                                    tracing::warn!(
                                                        target: "session.rpc",
                                                        error = %parse_err,
                                                        "failed to parse incoming delta"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::warn!(
                                    target: "session.rpc",
                                    error = %e,
                                    "UDS listener accept error; stopping"
                                );
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "session.rpc",
                        path = %socket_path.display(),
                        error = %e,
                        "failed to bind UDS listener"
                    );
                }
            }
        }

        #[cfg(all(windows, not(unix)))]
        {

            let session_id = socket_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let transport = NamedPipeTransport::new(session_id);
            match transport.server() {
                Ok(mut server) => {
                    tracing::debug!(
                        target: "session.rpc",
                        pipe = %transport.pipe_name,
                        "named-pipe listener ready"
                    );
                    loop {
                        if let Err(e) = server.connect().await {
                            tracing::warn!(
                                target: "session.rpc",
                                error = %e,
                                "named-pipe connect error; stopping"
                            );
                            break;
                        }
                        let mut buf = Vec::new();
                        if server.read_to_end(&mut buf).await.is_ok() {
                            if let Ok(remote) = serde_json::from_slice::<RemoteDelta>(&buf) {
                                actor.apply_remote(remote);
                            }
                        }

                        match transport.server() {
                            Ok(next) => server = next,
                            Err(e) => {
                                tracing::warn!(
                                    target: "session.rpc",
                                    error = %e,
                                    "failed to recreate named-pipe server; stopping"
                                );
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "session.rpc",
                        error = %e,
                        "failed to create named-pipe server"
                    );
                }
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = (actor, socket_path);
            tracing::debug!(
                target: "session.rpc",
                "cross-process session sync not available on this platform"
            );
        }
    })
    .into_inner()
}

pub async fn send_delta_to_peer(
    socket_path: &std::path::Path,
    delta: &SessionDelta,
) -> std::io::Result<()> {
    let remote = RemoteDelta {
        source_session_id: String::new(),
        last_seen_seq: delta.seq.saturating_sub(1),
        delta: delta.clone(),
    };
    send_remote_delta_to_peer(socket_path, &remote).await
}

pub async fn send_remote_delta_to_peer(
    socket_path: &std::path::Path,
    remote: &RemoteDelta,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(remote)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    #[cfg(unix)]
    {
        let mut stream = tokio::net::UnixStream::connect(socket_path).await?;
        stream.write_all(&payload).await?;
        stream.shutdown().await?;
        session_write_mode_metrics::incr_session_rpc_send();
    }

    #[cfg(all(windows, not(unix)))]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        let pipe_name = format!(
            r"\\.\pipe\sen_session_{}",
            socket_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
        );
        let mut client = ClientOptions::new().open(&pipe_name)?;
        client.write_all(&payload).await?;
        session_write_mode_metrics::incr_session_rpc_send();
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (socket_path, payload);
    }

    Ok(())
}
