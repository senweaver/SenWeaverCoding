// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
#[cfg(unix)]
use tracing::error;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RpcTransport {

    #[default]
    Stdio,

    #[cfg(unix)]
    UnixSocket {

        path: PathBuf,

        #[serde(default = "default_socket_mode")]
        mode: String,
    },

    Http {

        #[serde(default = "default_http_host")]
        host: String,

        #[serde(default = "default_http_port")]
        port: u16,
    },
}

#[cfg(unix)]
fn default_socket_mode() -> String {
    "0755".to_string()
}

fn default_http_host() -> String {
    "127.0.0.1".to_string()
}

fn default_http_port() -> u16 {
    42618
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RpcServerConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub transport: RpcTransport,

    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,

    #[serde(default = "default_session_timeout")]
    pub session_timeout_secs: u64,

    #[serde(default = "default_socket_path")]
    pub default_socket_path: PathBuf,

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

pub fn build_transport(cfg: &RpcConfig) -> Result<RpcTransport> {
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
        let _ = path;
    }

    if let Some(ref http_cfg) = cfg.http {
        transports.push("http");
        let _ = http_cfg;
    }

    match transports.len() {
        0 => Ok(RpcTransport::Stdio),
        1 => {
            if cfg.stdio {
                Ok(RpcTransport::Stdio)
            } else if cfg.unix_socket.is_some() {
                #[cfg(unix)]
                {
                    let socket_path = cfg.unix_socket.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("rpc.unix_socket is expected to be set here")
                    })?;
                    Ok(RpcTransport::UnixSocket {
                        path: PathBuf::from(socket_path),
                        mode: default_socket_mode(),
                    })
                }
                #[cfg(not(unix))]
                {
                    Ok(RpcTransport::Stdio)
                }
            } else {
                let http_cfg = cfg
                    .http
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("rpc.http is expected to be set here"))?;
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

pub struct RpcServer {
    config: RpcServerConfig,
    ctx: Arc<RpcCtx>,
}

fn host_is_loopback(host: &str) -> bool {
    let h = host.trim();
    if h.eq_ignore_ascii_case("localhost") || h == "127.0.0.1" || h == "::1" {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

impl RpcServer {

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

    pub async fn from_config(rpc_config: RpcServerConfig, agent_config: Config) -> Result<Self> {
        let ctx = RpcCtx::new(agent_config);
        ctx.init(rpc_config.max_sessions, rpc_config.session_timeout_secs)
            .await;
        Ok(Self {
            config: rpc_config,
            ctx: Arc::new(ctx),
        })
    }

    pub async fn run(self) -> Result<()> {
        if !self.config.enabled {
            info!("RPC server is disabled (set rpc.enabled = true to enable)");
            return Ok(());
        }

        info!(
            "RPC server starting (transport={:?}, max_sessions={}, timeout={}s)",
            self.config.transport, self.config.max_sessions, self.config.session_timeout_secs
        );

        let sessions = Arc::clone(&self.ctx.state);
        let timeout_secs = self.config.session_timeout_secs;
        let reaper_handle =
            crate::runtime::spawn_supervised("rpc.session_reaper", async move {
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

        let transport_result = match &self.config.transport {
            RpcTransport::Stdio => self.run_stdio().await,
            #[cfg(unix)]
            RpcTransport::UnixSocket { path, mode: _ } => self.run_unix_socket(path).await,
            RpcTransport::Http { host, port } => self.run_http(host, *port).await,
        };

        reaper_handle.abort();
        transport_result
    }

    async fn run_stdio(&self) -> Result<()> {
        info!("RPC: running in stdio mode");

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut stdout = tokio::io::stdout();
        let mut line = String::new();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        self.ctx.set_stdout(tx).await;

        let ctx = Arc::clone(&self.ctx);
        let stdout_tx = self.ctx.stdout_tx.clone();
        let reader_handle =
            crate::runtime::task_manager::spawn_supervised("rpc.stdio_reader", async move {
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
                                    let ctx_req = Arc::clone(&ctx);
                                    tokio::spawn(async move {
                                        use futures_util::FutureExt as _;
                                        let method = req.method.clone();
                                        let id = req.id.clone().unwrap_or(serde_json::Value::Null);
                                        let outcome = std::panic::AssertUnwindSafe(
                                            ctx_req.handle_request(
                                                &req.method,
                                                req.params,
                                                req.id,
                                            ),
                                        )
                                        .catch_unwind()
                                        .await;
                                        if let Err(panic_payload) = outcome {
                                            let description = crate::util::describe_panic(
                                                panic_payload.as_ref(),
                                            );
                                            tracing::error!(
                                                method = %method,
                                                panic = %description,
                                                "RPC request handler panicked"
                                            );
                                            ctx_req
                                                .write_error(
                                                    id,
                                                    crate::rpc::codec::RpcError::internal(
                                                        format!(
                                                            "request handler panicked: {description}"
                                                        ),
                                                    ),
                                                )
                                                .await;
                                        }
                                    });
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
                                        match serde_json::to_string(&resp) {
                                            Ok(s) => {
                                                let _ = tx.send(s + "\n").await;
                                            }
                                            Err(serialize_err) => {
                                                tracing::error!(
                                                    "RPC: failed to serialize parse-error response: {serialize_err}"
                                                );
                                            }
                                        }
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

        let writer_handle =
            crate::runtime::task_manager::spawn_supervised("rpc.stdio_writer", async move {
                while let Some(line) = rx.recv().await {
                    if stdout.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    if stdout.flush().await.is_err() {
                        break;
                    }
                }
            });

        reader_handle.into_inner().await?;
        writer_handle.abort();
        Ok(())
    }

    #[cfg(unix)]
    async fn run_unix_socket(&self, socket_path: &PathBuf) -> Result<()> {
        use tokio::net::UnixListener;

        info!("RPC: running in Unix Socket mode at {:?}", socket_path);

        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
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
                    crate::runtime::spawn_supervised("rpc.uds_connection", async move {
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
        let parse_err_tx = tx.clone();
        let ctx = ctx.with_output(tx);

        let writer_handle =
            crate::runtime::task_manager::spawn_supervised("rpc.uds_writer", async move {
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

        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(req) => {
                    let ctx_req = Arc::clone(&ctx);
                    crate::runtime::spawn_supervised("rpc.uds_request", async move {
                        use futures_util::FutureExt as _;
                        let method = req.method.clone();
                        let id = req.id.clone().unwrap_or(serde_json::Value::Null);
                        let outcome = std::panic::AssertUnwindSafe(ctx_req.handle_request(
                            &req.method,
                            req.params,
                            req.id,
                        ))
                        .catch_unwind()
                        .await;
                        if let Err(panic_payload) = outcome {
                            let description =
                                crate::util::describe_panic(panic_payload.as_ref());
                            tracing::error!(
                                method = %method,
                                panic = %description,
                                "RPC UDS request handler panicked"
                            );
                            ctx_req
                                .write_error(
                                    id,
                                    crate::rpc::codec::RpcError::internal(format!(
                                        "request handler panicked: {description}"
                                    )),
                                )
                                .await;
                        }
                    });
                }
                Err(e) => {
                    warn!("RPC UDS: failed to parse request: {e}");
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32700, "message": format!("Parse error: {e}") },
                        "id": null,
                    });
                    let payload = serde_json::to_string(&resp).unwrap_or_default();
                    let _ = parse_err_tx.send(payload).await;
                }
            }
        }

        drop(parse_err_tx);
        writer_handle.abort();
        Ok(())
    }

    async fn run_http(&self, host: &str, port: u16) -> Result<()> {
        use axum::{
            Json, Router, body::Body, extract::State, http::StatusCode, response::IntoResponse,
            routing::post,
        };
        use std::net::SocketAddr;

        info!("RPC: running in HTTP mode on {}:{}", host, port);
        let token_present = crate::util::get_runtime_var("SEN_RPC_TOKEN")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !host_is_loopback(host) && !token_present {
            anyhow::bail!(
                "RPC HTTP transport refused: host '{host}' is not loopback and SEN_RPC_TOKEN is \
                 not set. Bind to 127.0.0.1, or set SEN_RPC_TOKEN to expose it with Bearer auth."
            );
        }
        if !token_present {
            tracing::warn!(
                "SECURITY: RPC HTTP transport is running WITHOUT authentication \
                 (SEN_RPC_TOKEN not set) on loopback; mutating methods (session/*, tool/exec, \
                 memory/store, blackboard writes) are REFUSED until a token is configured. \
                 Set SEN_RPC_TOKEN to enable them behind Bearer auth."
            );
        }

        let shared_ctx = Arc::clone(&self.ctx);

        async fn handle_rpc(
            State(ctx): State<Arc<RpcCtx>>,
            headers: axum::http::HeaderMap,
            Json(body): Json<serde_json::Value>,
        ) -> impl IntoResponse {
            if let Some(expected) = crate::util::get_runtime_var("SEN_RPC_TOKEN")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            {
                let presented = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("Bearer "))
                    .map(str::trim)
                    .unwrap_or("");
                if !crate::security::pairing::constant_time_eq(presented, &expected) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        [("content-type", "application/json")],
                        Body::from(
                            serde_json::json!({
                                "jsonrpc": "2.0",
                                "error": { "code": -32007, "message": "Unauthorized" },
                                "id": serde_json::Value::Null,
                            })
                            .to_string(),
                        ),
                    );
                }
            }
            let token_configured = crate::util::get_runtime_var("SEN_RPC_TOKEN")
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            const MUTATING_METHODS: &[&str] = &[
                "session/new",
                "session/prompt",
                "session/prompt_stream",
                "session/stop",
                "session/kill",
                "tool/exec",
                "memory/store",
                "blackboard/put",
                "blackboard/watch",
                "blackboard/unwatch",
            ];

            let requests: Vec<serde_json::Value> = match body.as_array() {
                Some(arr) => arr.clone(),
                None => vec![body],
            };

            let mut responses = Vec::new();
            for req_value in requests {
                match serde_json::from_value::<JsonRpcRequest>(req_value) {
                    Ok(req) => {
                        let id = req.id.clone().unwrap_or(serde_json::Value::Null);
                        let normalized_method = req.method.replace('.', "/");
                        if !token_configured
                            && MUTATING_METHODS.contains(&normalized_method.as_str())
                        {
                            responses.push(serde_json::json!({
                                "jsonrpc": "2.0",
                                "error": {
                                    "code": -32007,
                                    "message": format!(
                                        "Method '{}' is disabled on the unauthenticated RPC HTTP transport. \
                                         Set SEN_RPC_TOKEN and send it as a Bearer token to enable mutating methods.",
                                        req.method
                                    ),
                                },
                                "id": id,
                            }));
                            continue;
                        }
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

            let serialize_target = if responses.len() == 1 {
                serde_json::to_string(&responses[0])
            } else {
                serde_json::to_string(&responses)
            };
            let body = match serialize_target {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("RPC HTTP: failed to serialize response: {e}");
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("Internal serialization error: {e}"),
                        },
                        "id": serde_json::Value::Null,
                    })
                    .to_string()
                }
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
