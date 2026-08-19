// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::observability::session_write_mode_metrics;
use crate::session::state::{RemoteDelta, SessionActor, SessionDelta};

#[derive(serde::Serialize, serde::Deserialize)]
struct AuthEnvelope {
    auth: String,
    remote: RemoteDelta,
}

fn secret_path_for(socket_path: &Path) -> PathBuf {
    let mut s = socket_path.as_os_str().to_os_string();
    s.push(".secret");
    PathBuf::from(s)
}

fn ensure_session_rpc_secret(socket_path: &Path) -> std::io::Result<String> {
    let path = secret_path_for(socket_path);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    crate::util::atomic_write(&path, token.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    match std::fs::read_to_string(&path) {
        Ok(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        _ => Ok(token),
    }
}

fn secrets_match(a: &str, b: &str) -> bool {
    crate::security::pairing::constant_time_eq(a, b)
}

#[async_trait::async_trait]
pub trait SessionRpcTransport: Send + Sync + 'static {

    async fn send(&self, session_id: &str, delta: &SessionDelta) -> std::io::Result<()>;
}

pub fn rpc_socket_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".sen").join("session-rpc")
}

pub fn rpc_socket_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    rpc_socket_dir(workspace_root).join(session_id)
}

static SESSION_SOCKET_REGISTRY: once_cell::sync::Lazy<
    parking_lot::RwLock<std::collections::HashMap<String, PathBuf>>,
> = once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(std::collections::HashMap::new()));

fn register_session_socket(session_id: &str, socket_path: PathBuf) {
    SESSION_SOCKET_REGISTRY
        .write()
        .insert(session_id.to_string(), socket_path);
}

fn lookup_session_socket(session_id: &str) -> Option<PathBuf> {
    SESSION_SOCKET_REGISTRY.read().get(session_id).cloned()
}

pub struct PeerSocketTransport;

impl PeerSocketTransport {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for PeerSocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SessionRpcTransport for PeerSocketTransport {
    async fn send(&self, session_id: &str, delta: &SessionDelta) -> std::io::Result<()> {
        let Some(path) = lookup_session_socket(session_id) else {
            return Ok(());
        };
        send_delta_to_peer(&path, delta).await
    }
}

pub fn enable_cross_process_sync(
    workspace_root: &Path,
    session_id: &str,
    actor: &Arc<SessionActor>,
) {
    let dir = rpc_socket_dir(workspace_root);
    let _ = std::fs::create_dir_all(&dir);
    let socket = dir.join(session_id);
    register_session_socket(session_id, socket.clone());
    let actor_clone = Arc::clone(actor);
    let session_for_log = session_id.to_string();
    crate::runtime::spawn_supervised("session.rpc.bootstrap", async move {
        let _ = spawn_rpc_listener(actor_clone, socket).await;
        tracing::debug!(
            target: "session.rpc",
            session_id = %session_for_log,
            "cross-process session sync listener requested"
        );
    });
    static TRANSPORT_INSTALLED: std::sync::Once = std::sync::Once::new();
    TRANSPORT_INSTALLED.call_once(move || {
        crate::session::sync::SessionSyncHub::global()
            .with_transport(Arc::new(PeerSocketTransport::new()));
    });
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
        let listener_secret = ensure_session_rpc_secret(&socket_path).unwrap_or_default();
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
                                let expected = listener_secret.clone();
                                crate::runtime::spawn_supervised("session.rpc.conn", async move {
                                    let mut buf = Vec::new();
                                    if stream.read_to_end(&mut buf).await.is_ok() {
                                        apply_authenticated_delta(&actor, &buf, &expected);
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
                            apply_authenticated_delta(&actor, &buf, &listener_secret);
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

fn apply_authenticated_delta(actor: &Arc<SessionActor>, buf: &[u8], expected: &str) {
    match serde_json::from_slice::<AuthEnvelope>(buf) {
        Ok(env) if !expected.is_empty() && secrets_match(&env.auth, expected) => {
            actor.apply_remote(env.remote);
        }
        Ok(_) => {
            tracing::warn!(
                target: "session.rpc",
                "dropped session delta with missing/incorrect auth token"
            );
        }
        Err(parse_err) => {
            tracing::warn!(
                target: "session.rpc",
                error = %parse_err,
                "failed to parse authenticated session delta envelope"
            );
        }
    }
}

pub async fn send_remote_delta_to_peer(
    socket_path: &std::path::Path,
    remote: &RemoteDelta,
) -> std::io::Result<()> {
    let auth = ensure_session_rpc_secret(socket_path)?;
    let envelope = AuthEnvelope {
        auth,
        remote: remote.clone(),
    };
    let payload = serde_json::to_vec(&envelope)
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
