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

pub fn process_instance_id() -> &'static str {
    static INSTANCE: once_cell::sync::Lazy<String> =
        once_cell::sync::Lazy::new(|| uuid::Uuid::new_v4().simple().to_string());
    &INSTANCE
}

fn short_instance_id() -> &'static str {
    &process_instance_id()[..12.min(process_instance_id().len())]
}

fn session_secret_path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.secret"))
}

fn ensure_session_rpc_secret(dir: &Path, session_id: &str) -> std::io::Result<String> {
    let path = session_secret_path(dir, session_id);
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
    rpc_socket_dir(workspace_root).join(format!("{session_id}.{}", short_instance_id()))
}

fn peer_endpoint_prefix(session_id: &str) -> String {
    format!("{session_id}.")
}

fn instance_from_endpoint(file_name: &str, session_id: &str) -> Option<String> {
    let rest = file_name.strip_prefix(&peer_endpoint_prefix(session_id))?;
    let instance = rest.strip_suffix(".pipe").unwrap_or(rest);
    if instance.is_empty() || instance == "secret" || instance.contains('.') {
        return None;
    }
    Some(instance.to_string())
}

static SESSION_RPC_DIRS: once_cell::sync::Lazy<
    parking_lot::RwLock<std::collections::HashMap<String, PathBuf>>,
> = once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(std::collections::HashMap::new()));

fn register_session_rpc_dir(session_id: &str, dir: PathBuf) {
    SESSION_RPC_DIRS
        .write()
        .insert(session_id.to_string(), dir);
}

fn lookup_session_rpc_dir(session_id: &str) -> Option<PathBuf> {
    SESSION_RPC_DIRS.read().get(session_id).cloned()
}

const PEER_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(2);

type PeerList = Vec<(String, PathBuf)>;

static PEER_ENUM_CACHE: once_cell::sync::Lazy<
    parking_lot::Mutex<std::collections::HashMap<String, (std::time::Instant, PeerList)>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn cached_peer_instances(dir: &Path, session_id: &str) -> PeerList {
    {
        let cache = PEER_ENUM_CACHE.lock();
        if let Some((refreshed_at, peers)) = cache.get(session_id) {
            if refreshed_at.elapsed() < PEER_CACHE_TTL {
                return peers.clone();
            }
        }
    }
    let peers = enumerate_peer_instances(dir, session_id);
    PEER_ENUM_CACHE.lock().insert(
        session_id.to_string(),
        (std::time::Instant::now(), peers.clone()),
    );
    peers
}

fn invalidate_peer_cache(session_id: &str) {
    PEER_ENUM_CACHE.lock().remove(session_id);
}

static SESSION_SECRET_CACHE: once_cell::sync::Lazy<
    parking_lot::Mutex<std::collections::HashMap<String, String>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn cached_session_secret(dir: &Path, session_id: &str) -> std::io::Result<String> {
    if let Some(secret) = SESSION_SECRET_CACHE.lock().get(session_id) {
        return Ok(secret.clone());
    }
    let secret = ensure_session_rpc_secret(dir, session_id)?;
    SESSION_SECRET_CACHE
        .lock()
        .insert(session_id.to_string(), secret.clone());
    Ok(secret)
}

fn enumerate_peer_instances(dir: &Path, session_id: &str) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let own = short_instance_id();
    let mut peers = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.ends_with(".secret") {
            continue;
        }
        let Some(instance) = instance_from_endpoint(file_name, session_id) else {
            continue;
        };
        if instance == own {
            continue;
        }
        peers.push((instance, path));
    }
    peers
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
        let Some(dir) = lookup_session_rpc_dir(session_id) else {
            return Ok(());
        };
        let peers = cached_peer_instances(&dir, session_id);
        if peers.is_empty() {
            return Ok(());
        }
        let remote = RemoteDelta {
            source_session_id: process_instance_id().to_string(),
            last_seen_seq: delta.version.saturating_sub(1),
            delta: delta.clone(),
        };
        let auth = cached_session_secret(&dir, session_id)?;
        for (instance, endpoint) in peers {
            if let Err(e) =
                send_remote_delta_to_instance(&endpoint, session_id, &instance, &auth, &remote)
                    .await
            {
                tracing::debug!(
                    target: "session.rpc",
                    session_id = %session_id,
                    peer_instance = %instance,
                    error = %e,
                    "peer delta send failed; removing stale endpoint marker"
                );
                let _ = std::fs::remove_file(&endpoint);
                invalidate_peer_cache(session_id);
            }
        }
        Ok(())
    }
}

pub fn enable_cross_process_sync(
    workspace_root: &Path,
    session_id: &str,
    actor: &Arc<SessionActor>,
) {
    let dir = rpc_socket_dir(workspace_root);
    let _ = std::fs::create_dir_all(&dir);
    register_session_rpc_dir(session_id, dir.clone());
    let socket = rpc_socket_path(workspace_root, session_id);
    let actor_clone = Arc::clone(actor);
    let session_for_log = session_id.to_string();
    let dir_for_listener = dir;
    let session_for_listener = session_id.to_string();
    crate::runtime::spawn_supervised("session.rpc.bootstrap", async move {
        let _ = spawn_rpc_listener(
            actor_clone,
            socket,
            dir_for_listener,
            session_for_listener,
        )
        .await;
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
fn windows_pipe_name(session_id: &str, instance: &str) -> String {
    format!(r"\\.\pipe\sen_session_{session_id}_{instance}")
}

pub async fn spawn_rpc_listener(
    actor: Arc<SessionActor>,
    socket_path: PathBuf,
    rpc_dir: PathBuf,
    session_id: String,
) -> tokio::task::JoinHandle<()> {
    crate::runtime::spawn_supervised("session.rpc.listener", async move {
        let listener_secret =
            ensure_session_rpc_secret(&rpc_dir, &session_id).unwrap_or_default();
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
            let marker_path = {
                let mut name = socket_path.as_os_str().to_os_string();
                name.push(".pipe");
                PathBuf::from(name)
            };
            if crate::util::atomic_write(
                &marker_path,
                std::process::id().to_string().as_bytes(),
            )
            .is_err()
            {
                tracing::warn!(
                    target: "session.rpc",
                    path = %marker_path.display(),
                    "failed to write named-pipe endpoint marker; peers cannot discover this listener"
                );
            }
            let pipe_name = windows_pipe_name(&session_id, short_instance_id());
            let make_server = || {
                tokio::net::windows::named_pipe::ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&pipe_name)
            };
            match make_server() {
                Ok(mut server) => {
                    tracing::debug!(
                        target: "session.rpc",
                        pipe = %pipe_name,
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

                        match tokio::net::windows::named_pipe::ServerOptions::new()
                            .create(&pipe_name)
                        {
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
            let _ = std::fs::remove_file(&marker_path);
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = (actor, socket_path, rpc_dir, session_id);
            tracing::debug!(
                target: "session.rpc",
                "cross-process session sync not available on this platform"
            );
        }
    })
    .into_inner()
}

fn apply_authenticated_delta(actor: &Arc<SessionActor>, buf: &[u8], expected: &str) {
    match serde_json::from_slice::<AuthEnvelope>(buf) {
        Ok(env) if !expected.is_empty() && secrets_match(&env.auth, expected) => {
            if env.remote.source_session_id == process_instance_id() {
                tracing::debug!(
                    target: "session.rpc",
                    "dropping self-originated session delta (loopback)"
                );
                return;
            }
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

async fn send_remote_delta_to_instance(
    endpoint: &Path,
    session_id: &str,
    instance: &str,
    auth: &str,
    remote: &RemoteDelta,
) -> std::io::Result<()> {
    let envelope = AuthEnvelope {
        auth: auth.to_string(),
        remote: remote.clone(),
    };
    let payload = serde_json::to_vec(&envelope)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    #[cfg(unix)]
    {
        let _ = (session_id, instance);
        let mut stream = tokio::net::UnixStream::connect(endpoint).await?;
        stream.write_all(&payload).await?;
        stream.shutdown().await?;
        session_write_mode_metrics::incr_session_rpc_send();
    }

    #[cfg(all(windows, not(unix)))]
    {
        let _ = endpoint;
        use tokio::net::windows::named_pipe::ClientOptions;
        let pipe_name = windows_pipe_name(session_id, instance);
        let mut client = ClientOptions::new().open(&pipe_name)?;
        client.write_all(&payload).await?;
        session_write_mode_metrics::incr_session_rpc_send();
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (endpoint, session_id, instance, payload);
    }

    Ok(())
}
