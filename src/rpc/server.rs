// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! Multi-transport RPC server for SenWeaverCoding.
//!
//! Supports three transport modes:
//! - **Stdio**: JSON-RPC over stdin/stdout (IDE integration, subprocess)
//! - **UnixSocket**: Unix Domain Socket (local Python/CLI clients) — Unix only
//! - **Http**: HTTP/JSON-RPC server (network clients, microservices)
//!
//! ## Usage
//!
//! ```rust,ignore
//! let config = RpcServerConfig::default();
//! let server = RpcServer::new(config, sen_config).await?;
//! server.run().await?;
//! ```
use crate::config::schema::{Config, RpcConfig};
use crate::rpc::codec::JsonRpcRequest;
use crate::rpc::methods::RpcCtx;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{Duration, interval};
use tracing::{debug, info, warn};

/// Transport mode for the RPC server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RpcTransport {
    /// JSON-RPC over stdin/stdout. Ideal for subprocess/IDE integration.
    #[default]
    Stdio,
    /// Unix Domain Socket. Ideal for local Python/CLI clients. Unix-only.
    #[cfg(unix)]
    UnixSocket {
        /// Path to the socket file.
        path: PathBuf,
        /// Unix permission mode (e.g. "0777"). Defaults to "0755".
        #[serde(default = "default_socket_mode")]
        mode: String,
    },
    /// HTTP server on TCP. Ideal for network clients.
    Http {
        /// Host to bind to.
        #[serde(default = "default_http_host")]
        host: String,
        /// Port to listen on.
        #[serde(default = "default_http_port")]
        port: u16,
    },
}

fn default_socket_mode() -> String {
    "0755".to_string()
}

fn default_http_host() -> String {
    "127.0.0.1".to_string()
}

fn default_http_port() -> u16 {
    42618
}

/// RPC server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RpcServerConfig {
    /// Enable the RPC server.
    #[serde(default)]
    pub enabled: bool,
    /// Transport mode.
    #[serde(default)]
    pub transport: RpcTransport,
    /// Maximum concurrent sessions.
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
    /// Session inactivity timeout in seconds.
    #[serde(default = "default_session_timeout")]
    pub session_timeout_secs: u64,
    /// Default socket path for UnixSocket transport.
    #[serde(default = "default_socket_path")]
    pub default_socket_path: PathBuf,
    /// Default HTTP port for Http transport.
    #[serde(default = "default_http_port")]
    pub default_http_port: u16,
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            transport: RpcTransport::Stdio,
            max_sessions: default_max_sessions(),
            session_timeout_secs: default_session_timeout(),
            default_socket_path: default_socket_path(),
            default_http_port: default_http_port(),
        }
    }
}

fn default_max_sessions() -> usize {
    10
}

fn default_session_timeout() -> u64 {
    3600
}

#[cfg(unix)]
fn default_socket_path() -> PathBuf {
    PathBuf::from("/tmp/sen-rpc.sock")
}

#[cfg(windows)]
fn default_socket_path() -> PathBuf {
    PathBuf::from(r"\\.\pipe\sen")
}

/// Derive the [`RpcTransport`] from the top-level [`RpcConfig`].
pub(crate) fn build_transport(cfg: &RpcConfig) -> Result<RpcTransport> {
    let mut transports = Vec::new();

    if cfg.stdio {
        transports.push("stdio");
    }

    if let Some(ref path) = cfg.unix_socket {
        #[cfg(unix)]
        {
            transports.push("unix_socket");
        }
        #[cfg(not(unix))]
        {
            tracing::warn!(
                "rpc.unix_socket is set but this platform does not support Unix Domain Sockets; ignoring"
            );
        }
        let _ = path; // silence unused warning on Windows
    }

    if let Some(ref http_cfg) = cfg.http {
        transports.push("http");
        let _ = http_cfg; // silence unused warning
    }

    match transports.len() {
        0 => Ok(RpcTransport::Stdio),
        1 => {
            if cfg.stdio {
                Ok(RpcTransport::Stdio)
            } else if cfg.unix_socket.is_some() {
                #[cfg(unix)]
                {
                    Ok(RpcTransport::UnixSocket {
                        path: PathBuf::from(cfg.unix_socket.as_ref().unwrap()),
                        mode: default_socket_mode(),
                    })
                }
                #[cfg(not(unix))]
                {
                    Ok(RpcTransport::Stdio)
                }
            } else {
                let http_cfg = cfg.http.as_ref().unwrap();
                Ok(RpcTransport::Http {
                    host: http_cfg.host.clone(),
                    port: http_cfg.port,
                })
            }
        }
        _ => {
            tracing::warn!(
                "Multiple RPC transports enabled ({}); using stdio",
                transports.join("+")
            );
            Ok(RpcTransport::Stdio)
        }
    }
}

/// The RPC server itself.
pub struct RpcServer {
    config: RpcServerConfig,
    ctx: Arc<RpcCtx>,
}

impl RpcServer {
    /// Create a new RPC server with the given config.
    /// Build a new RPC server from the global [`Config`].
    pub async fn new(config: &Config) -> Result<Self> {
        let rpc_cfg = &config.rpc;
        let ctx = RpcCtx::new(config.clone());

        let server_config = RpcServerConfig {
            enabled: rpc_cfg.enabled,
            transport: build_transport(rpc_cfg)?,
            max_sessions: rpc_cfg.max_sessions,
            session_timeout_secs: rpc_cfg.session_timeout_secs,
            default_socket_path: default_socket_path(),
            default_http_port: default_http_port(),
        };

        ctx.init(
            server_config.max_sessions,
            server_config.session_timeout_secs,
        )
        .await;
        Ok(Self {
            config: server_config,
            ctx: Arc::new(ctx),
        })
    }

    /// Build a new RPC server from an explicit [`RpcServerConfig`] + [`Config`].
    pub async fn from_config(rpc_config: RpcServerConfig, agent_config: Config) -> Result<Self> {
        let ctx = RpcCtx::new(agent_config);
        ctx.init(rpc_config.max_sessions, rpc_config.session_timeout_secs)
            .await;
        Ok(Self {
            config: rpc_config,
            ctx: Arc::new(ctx),
        })
    }

    /// Run the server in the configured transport mode (consumes self).
    pub async fn run(self) -> Result<()> {
        if !self.config.enabled {
            info!("RPC server is disabled (set rpc.enabled = true to enable)");
            return Ok(());
        }

        info!(
            "RPC server starting (transport={:?}, max_sessions={}, timeout={}s)",
            self.config.transport, self.config.max_sessions, self.config.session_timeout_secs
        );

        // Spawn the session reaper
        let sessions = Arc::clone(&self.ctx.state);
        let timeout_secs = self.config.session_timeout_secs;
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;
                let state_guard = sessions.read().await;
                if let Some(ref state) = *state_guard {
                    let mut sessions = state.sessions.lock().await;
                    let before = sessions.len();
                    let deadline = Duration::from_secs(timeout_secs);
                    sessions.retain(|id, session| {
                        let expired = session.last_active.elapsed() > deadline;
                        if expired {
                            info!("RPC: session {id} expired after inactivity");
                        }
                        !expired
                    });
                    let reaped = before - sessions.len();
                    if reaped > 0 {
                        debug!("RPC: reaped {reaped} expired session(s)");
                    }
                }
            }
        });

        match &self.config.transport {
            RpcTransport::Stdio => {
                self.run_stdio().await?;
            }
            #[cfg(unix)]
            RpcTransport::UnixSocket { path, mode: _ } => {
                self.run_unix_socket(path).await?;
            }
            RpcTransport::Http { host, port } => {
                self.run_http(host, *port).await?;
            }
        }

        Ok(())
    }

    // ── Stdio transport ───────────────────────────────────────────────────

    /// Run in stdio mode (JSON-RPC over stdin/stdout).
    async fn run_stdio(&self) -> Result<()> {
        info!("RPC: running in stdio mode");

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut stdout = tokio::io::stdout();
        let mut line = String::new();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        self.ctx.set_stdout(tx).await;

        // Reader task: parse and dispatch incoming requests
        let ctx = Arc::clone(&self.ctx);
        let stdout_tx = self.ctx.stdout_tx.clone();
        let reader_handle = tokio::spawn(async move {
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        info!("RPC stdio: stdin closed");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                            Ok(req) => {
                                ctx.handle_request(&req.method, req.params, req.id).await;
                            }
                            Err(e) => {
                                warn!("RPC: failed to parse JSON-RPC request: {e}");
                                let resp = serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "error": {
                                        "code": -32700,
                                        "message": format!("Parse error: {e}"),
                                    },
                                    "id": null,
                                });
                                let guard = stdout_tx.lock().await;
                                if let Some(ref tx) = *guard {
                                    let _ =
                                        tx.send(serde_json::to_string(&resp).unwrap() + "\n").await;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("RPC stdio: read error: {e}");
                        break;
                    }
                }
            }
        });

        // Writer task: drain the notification channel directly to stdout
        let writer_handle = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if stdout.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if stdout.flush().await.is_err() {
                    break;
                }
            }
        });

        reader_handle.await?;
        writer_handle.abort();
        Ok(())
    }

    // ── Unix Socket transport (Unix only) ─────────────────────────────────

    /// Run in Unix Socket mode.
    #[cfg(unix)]
    async fn run_unix_socket(&self, socket_path: &PathBuf) -> Result<()> {
        use tokio::net::UnixListener;

        info!("RPC: running in Unix Socket mode at {:?}", socket_path);

        // Remove stale socket file
        if socket_path.exists() {
            std::os::unix::fs::remove_socket(socket_path)?;
        }

        let listener = UnixListener::bind(socket_path)?;
        info!("RPC Unix Socket: listening on {:?}", socket_path);

        {
            use std::os::unix::fs::PermissionsExt;
            let mode = 0o755u32;
            if let Ok(metadata) = socket_path.metadata() {
                let mut perms = metadata.permissions();
                perms.set_mode(mode);
                std::fs::set_permissions(socket_path, perms)?;
            }
        }

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let ctx = Arc::clone(&self.ctx);
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_uds_stream(ctx, stream).await {
                            warn!("RPC UDS: connection error: {e}");
                        }
                    });
                }
                Err(e) => {
                    error!("RPC UDS: accept error: {e}");
                }
            }
        }
    }

    #[cfg(unix)]
    async fn handle_uds_stream(ctx: Arc<RpcCtx>, stream: tokio::net::UnixStream) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        ctx.set_stdout(tx).await;

        let writer_handle = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        while let Some(line) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(req) => {
                    ctx.handle_request(&req.method, req.params, req.id).await;
                }
                Err(e) => {
                    warn!("RPC UDS: failed to parse request: {e}");
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32700, "message": format!("Parse error: {e}") },
                        "id": null,
                    });
                    let _ = writer
                        .write_all(serde_json::to_string(&resp).unwrap().as_bytes())
                        .await;
                    let _ = writer.write_all(b"\n").await;
                    let _ = writer.flush().await;
                }
            }
        }

        writer_handle.abort();
        Ok(())
    }

    // ── HTTP transport ───────────────────────────────────────────────────

    /// Run in HTTP mode (JSON-RPC over HTTP POST).
    async fn run_http(&self, host: &str, port: u16) -> Result<()> {
        use axum::{
            Json, Router, body::Body, extract::State, http::StatusCode, response::IntoResponse,
            routing::post,
        };
        use std::net::SocketAddr;

        info!("RPC: running in HTTP mode on {}:{}", host, port);

        let shared_ctx = Arc::clone(&self.ctx);

        async fn handle_rpc(
            State(ctx): State<Arc<RpcCtx>>,
            Json(body): Json<serde_json::Value>,
        ) -> impl IntoResponse {
            let requests: Vec<serde_json::Value> = if body.is_array() {
                body.as_array().unwrap().clone()
            } else {
                vec![body]
            };

            let mut responses = Vec::new();
            for req_value in requests {
                match serde_json::from_value::<JsonRpcRequest>(req_value) {
                    Ok(req) => {
                        let id = req.id.clone().unwrap_or(serde_json::Value::Null);
                        let result = ctx.handle_http_request(&req.method, req.params).await;
                        let resp = match result {
                            Ok(value) => serde_json::json!({
                                "jsonrpc": "2.0",
                                "result": value,
                                "id": id,
                            }),
                            Err(err) => serde_json::json!({
                                "jsonrpc": "2.0",
                                "error": {
                                    "code": err.code,
                                    "message": err.message,
                                    "data": err.data,
                                },
                                "id": id,
                            }),
                        };
                        responses.push(resp);
                    }
                    Err(e) => {
                        responses.push(serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": { "code": -32700, "message": format!("Parse error: {e}") },
                            "id": null,
                        }));
                    }
                }
            }

            let body = if responses.len() == 1 {
                serde_json::to_string(&responses[0]).unwrap()
            } else {
                serde_json::to_string(&responses).unwrap()
            };

            (
                StatusCode::OK,
                [("content-type", "application/json")],
                Body::from(body),
            )
        }

        let app = Router::new()
            .route("/", post(handle_rpc))
            .route("/rpc", post(handle_rpc))
            .with_state(shared_ctx);

        let addr: SocketAddr = format!("{host}:{port}").parse()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        info!("RPC HTTP: listening on {}", addr);

        axum::serve(listener, app).await?;

        Ok(())
    }
}
