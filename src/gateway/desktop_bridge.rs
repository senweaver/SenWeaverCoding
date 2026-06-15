// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::AppState;
use crate::tools::browser::{
    DockController, DockRequest, DockResponse, DockTabInfo,
};
use crate::tools::web::fetch::{FetchController, FetchedPage};

pub const BRIDGE_MODE_ENV: &str = "SEN_DESKTOP_BRIDGE";
pub const BRIDGE_TOKEN_ENV: &str = "SEN_DESKTOP_BRIDGE_TOKEN";

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(60);
const OUTBOUND_CAPACITY: usize = 256;
const RECONNECT_GRACE: Duration = Duration::from_secs(10);

pub fn bridge_mode() -> bool {
    std::env::var(BRIDGE_MODE_ENV)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

struct PendingEntry {
    responder: oneshot::Sender<Result<Value, String>>,
    frame: String,
    target: String,
    method: String,
}

fn is_replay_safe(target: &str, method: &str) -> bool {
    matches!(
        (target, method),
        ("dock", "list_tabs")
            | ("dock", "get_state")
            | ("dock", "screenshot")
            | ("dock", "ensure_visible")
            | ("dock", "present_session")
            | ("fetch", "fetch")
    )
}

struct BridgeHub {
    outbound: Mutex<Option<mpsc::Sender<String>>>,
    pending: Mutex<HashMap<u64, PendingEntry>>,
    next_id: AtomicU64,
    connection_epoch: AtomicU64,
}

impl BridgeHub {
    fn new() -> Self {
        Self {
            outbound: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            connection_epoch: AtomicU64::new(0),
        }
    }

    fn fail_all_pending(&self, reason: &str) {
        let drained: Vec<_> = {
            let mut guard = self.pending.lock();
            guard.drain().collect()
        };
        for (_, entry) in drained {
            let _ = entry.responder.send(Err(reason.to_string()));
        }
    }

    async fn replay_or_abort_pending(&self, sender: &mpsc::Sender<String>) {
        let drained: Vec<(u64, PendingEntry)> = {
            let mut guard = self.pending.lock();
            guard.drain().collect()
        };
        for (id, entry) in drained {
            if is_replay_safe(&entry.target, &entry.method) {
                let frame = entry.frame.clone();
                self.pending.lock().insert(id, entry);
                if sender.send(frame).await.is_err() {
                    if let Some(entry) = self.pending.lock().remove(&id) {
                        let _ = entry.responder.send(Err(
                            "desktop bridge reconnected but request replay failed".to_string(),
                        ));
                    }
                }
            } else {
                let _ = entry.responder.send(Err(format!(
                    "desktop bridge reconnected; non-idempotent request {}.{} aborted",
                    entry.target, entry.method
                )));
            }
        }
    }

    async fn request(
        &self,
        target: &str,
        method: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value> {
        let sender = self
            .outbound
            .lock()
            .clone()
            .ok_or_else(|| anyhow!("bridge disconnected"))?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = json!({
            "id": id,
            "target": target,
            "method": method,
            "args": args,
        })
        .to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(
            id,
            PendingEntry {
                responder: tx,
                frame: frame.clone(),
                target: target.to_string(),
                method: method.to_string(),
            },
        );
        if sender.send(frame).await.is_err() {
            self.pending.lock().remove(&id);
            bail!("desktop bridge connection closed while sending request");
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(err))) => bail!("desktop bridge error: {err}"),
            Ok(Err(_)) => bail!("desktop bridge dropped the request"),
            Err(_) => {
                self.pending.lock().remove(&id);
                bail!(
                    "desktop bridge request timed out after {}s ({target}.{method})",
                    timeout.as_secs()
                )
            }
        }
    }
}

fn hub() -> &'static BridgeHub {
    static HUB: OnceLock<BridgeHub> = OnceLock::new();
    HUB.get_or_init(BridgeHub::new)
}

pub fn install_remote_controllers() {
    if !bridge_mode() {
        return;
    }
    crate::tools::browser::install_dock_controller(Arc::new(RemoteDockController));
    crate::tools::web::fetch::install_fetch_controller(Arc::new(RemoteFetchController));
    tracing::info!(
        target: "gateway.desktop_bridge",
        "desktop bridge mode active: remote dock/fetch controllers installed"
    );
}

pub async fn handle_bridge_ws(
    State(_state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    if !bridge_mode() {
        return (axum::http::StatusCode::NOT_FOUND, "bridge mode disabled").into_response();
    }
    let expected = std::env::var(BRIDGE_TOKEN_ENV).unwrap_or_default();
    if !expected.is_empty() {
        let provided = params.get("token").map(String::as_str).unwrap_or("");
        if provided != expected {
            return (axum::http::StatusCode::UNAUTHORIZED, "invalid bridge token")
                .into_response();
        }
    }
    ws.on_upgrade(handle_bridge_socket)
}

async fn handle_bridge_socket(socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<String>(OUTBOUND_CAPACITY);

    {
        let mut guard = hub().outbound.lock();
        *guard = Some(tx.clone());
    }
    hub().connection_epoch.fetch_add(1, Ordering::SeqCst);
    hub().replay_or_abort_pending(&tx).await;
    tracing::info!(target: "gateway.desktop_bridge", "desktop bridge connected");

    let writer = crate::runtime::spawn_supervised("gateway.desktop_bridge.writer", async move {
        while let Some(frame) = rx.recv().await {
            if sink.send(Message::Text(frame.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = stream.next().await {
        let text = match message {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(id) = parsed.get("id").and_then(Value::as_u64) else {
            continue;
        };
        let Some(waiter) = hub().pending.lock().remove(&id) else {
            continue;
        };
        let ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if ok {
            let value = parsed.get("value").cloned().unwrap_or(Value::Null);
            let _ = waiter.responder.send(Ok(value));
        } else {
            let err = parsed
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown bridge error")
                .to_string();
            let _ = waiter.responder.send(Err(err));
        }
    }

    {
        let mut guard = hub().outbound.lock();
        *guard = None;
    }
    let disconnect_epoch = hub().connection_epoch.fetch_add(1, Ordering::SeqCst) + 1;
    crate::runtime::spawn_supervised("gateway.desktop_bridge.grace", async move {
        tokio::time::sleep(RECONNECT_GRACE).await;
        if hub().connection_epoch.load(Ordering::SeqCst) == disconnect_epoch
            && hub().outbound.lock().is_none()
        {
            hub().fail_all_pending("desktop bridge disconnected");
        }
    });
    writer.abort();
    tracing::info!(
        target: "gateway.desktop_bridge",
        grace_secs = RECONNECT_GRACE.as_secs(),
        "desktop bridge disconnected; holding pending requests for the reconnect grace period"
    );
}

struct RemoteDockController;

#[async_trait]
impl DockController for RemoteDockController {
    async fn ensure_visible(&self, session_hint: Option<String>) -> Result<()> {
        hub()
            .request(
                "dock",
                "ensure_visible",
                json!({ "session_hint": session_hint }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await
            .map(|_| ())
    }

    async fn exec(&self, req: DockRequest) -> Result<DockResponse> {
        let timeout = Duration::from_millis(req.timeout_ms.max(1_000)) + Duration::from_secs(5);
        let value = hub()
            .request(
                "dock",
                "exec",
                json!({
                    "kind": req.kind,
                    "args": req.args,
                    "timeout_ms": req.timeout_ms,
                }),
                timeout,
            )
            .await?;
        Ok(DockResponse {
            ok: value.get("ok").and_then(Value::as_bool).unwrap_or(false),
            value: value.get("value").cloned().unwrap_or(Value::Null),
            error: value
                .get("error")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        })
    }

    async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        use base64::Engine;
        let value = hub()
            .request(
                "dock",
                "screenshot",
                json!({ "full_page": full_page }),
                SCREENSHOT_TIMEOUT,
            )
            .await?;
        let b64 = value
            .get("png_base64")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("bridge screenshot response missing png_base64"))?;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| anyhow!("bridge screenshot base64 decode failed: {e}"))
    }

    async fn new_tab(&self, url: Option<String>, activate: bool) -> Result<u32> {
        let value = hub()
            .request(
                "dock",
                "new_tab",
                json!({ "url": url, "activate": activate }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        value
            .get("tab_id")
            .and_then(Value::as_u64)
            .map(|v| v as u32)
            .ok_or_else(|| anyhow!("bridge new_tab response missing tab_id"))
    }

    async fn close_tab(&self, tab_id: u32) -> Result<Option<u32>> {
        let value = hub()
            .request(
                "dock",
                "close_tab",
                json!({ "tab_id": tab_id }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        Ok(value
            .get("active")
            .and_then(Value::as_u64)
            .map(|v| v as u32))
    }

    async fn activate_tab(&self, tab_id: u32) -> Result<()> {
        hub()
            .request(
                "dock",
                "activate_tab",
                json!({ "tab_id": tab_id }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await
            .map(|_| ())
    }

    async fn list_tabs(&self) -> Result<Vec<DockTabInfo>> {
        let value = hub()
            .request("dock", "list_tabs", json!({}), DEFAULT_REQUEST_TIMEOUT)
            .await?;
        let tabs = value.get("tabs").cloned().unwrap_or(Value::Null);
        serde_json::from_value(tabs)
            .map_err(|e| anyhow!("bridge list_tabs response malformed: {e}"))
    }

    async fn bind_tab_to_session(&self, session_id: String, tab_id: u32) -> Result<()> {
        hub()
            .request(
                "dock",
                "bind_tab_to_session",
                json!({ "session_id": session_id, "tab_id": tab_id }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await
            .map(|_| ())
    }

    async fn unbind_tab_from_session(&self, session_id: String, tab_id: u32) -> Result<()> {
        hub()
            .request(
                "dock",
                "unbind_tab_from_session",
                json!({ "session_id": session_id, "tab_id": tab_id }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await
            .map(|_| ())
    }

    async fn release_agent_tabs_for_session(&self, session_id: String) -> Result<Vec<u32>> {
        let value = hub()
            .request(
                "dock",
                "release_agent_tabs_for_session",
                json!({ "session_id": session_id }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        Ok(value
            .get("tabs")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_u64)
                    .map(|v| v as u32)
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn present_session(&self, session_id: String) -> Result<Option<u32>> {
        let value = hub()
            .request(
                "dock",
                "present_session",
                json!({ "session_id": session_id }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        Ok(value
            .get("tab")
            .and_then(Value::as_u64)
            .map(|v| v as u32))
    }

    async fn park(&self) -> Result<()> {
        hub()
            .request("dock", "park", json!({}), DEFAULT_REQUEST_TIMEOUT)
            .await
            .map(|_| ())
    }
}

struct RemoteFetchController;

#[async_trait]
impl FetchController for RemoteFetchController {
    async fn fetch(&self, url: &str, timeout: Duration) -> Result<FetchedPage> {
        let value = hub()
            .request(
                "fetch",
                "fetch",
                json!({ "url": url, "timeout_ms": timeout.as_millis() as u64 }),
                timeout + Duration::from_secs(10),
            )
            .await?;
        Ok(FetchedPage {
            url: value
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or(url)
                .to_string(),
            title: value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
    }
}
