// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

const STALE_CONNECTING_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_INITIAL_DELAY: Duration = Duration::from_secs(2);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(30);
const WATCHDOG_POLL_INTERVAL: Duration = Duration::from_secs(1);
const REMOTE_BROADCAST_CAPACITY: usize = 1024;

fn sign_timestamp(secret: &str) -> (String, String) {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let ts = chrono::Utc::now().timestamp().to_string();
    let signature = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map(|mut mac| {
            mac.update(ts.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        })
        .unwrap_or_default();
    (ts, signature)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsState {
    Disconnected,
    Connecting,
    Connected,
    Closing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    pub msg_type: String,
    pub payload: serde_json::Value,
}

pub struct SessionWebSocket {
    url: String,
    auth_token: Option<String>,
    signing_secret: Option<String>,
    state: Arc<RwLock<WsState>>,
    connecting_since: Arc<parking_lot::Mutex<Option<Instant>>>,
    message_tx: broadcast::Sender<WsMessage>,
    outbox: mpsc::Sender<String>,
    out_rx: Arc<Mutex<mpsc::Receiver<String>>>,
    shutdown: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    reader_task: Arc<parking_lot::Mutex<Option<crate::runtime::TaskHandle>>>,
    writer_task: Arc<parking_lot::Mutex<Option<crate::runtime::TaskHandle>>>,
    supervisor_task: Arc<parking_lot::Mutex<Option<crate::runtime::TaskHandle>>>,
}

impl SessionWebSocket {
    pub fn new(url: &str) -> Self {
        Self::with_auth(url, None)
    }

    pub fn with_auth(url: &str, auth_token: Option<String>) -> Self {
        Self::with_auth_and_signing(url, auth_token, None)
    }

    pub fn with_auth_and_signing(
        url: &str,
        auth_token: Option<String>,
        signing_secret: Option<String>,
    ) -> Self {
        let (out_tx, out_rx) = mpsc::channel(256);
        let (message_tx, _) = broadcast::channel(REMOTE_BROADCAST_CAPACITY);
        Self {
            url: url.to_string(),
            auth_token,
            signing_secret: signing_secret.filter(|s| !s.trim().is_empty()),
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            connecting_since: Arc::new(parking_lot::Mutex::new(None)),
            message_tx,
            outbox: out_tx,
            out_rx: Arc::new(Mutex::new(out_rx)),
            shutdown: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            reader_task: Arc::new(parking_lot::Mutex::new(None)),
            writer_task: Arc::new(parking_lot::Mutex::new(None)),
            supervisor_task: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    fn abort_io_tasks(&self) {
        if let Some(handle) = self.reader_task.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.writer_task.lock().take() {
            handle.abort();
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsMessage> {
        self.message_tx.subscribe()
    }

    pub async fn state(&self) -> WsState {
        *self.state.read().await
    }

    pub async fn connect(&self) -> anyhow::Result<()> {
        {
            let mut state = self.state.write().await;
            match *state {
                WsState::Connected => return Ok(()),
                WsState::Connecting => {
                    let stale = match *self.connecting_since.lock() {
                        Some(since) => since.elapsed() >= STALE_CONNECTING_TIMEOUT,
                        None => true,
                    };
                    if !stale {
                        return Ok(());
                    }
                    tracing::warn!(
                        url = %self.url,
                        stale_after_secs = STALE_CONNECTING_TIMEOUT.as_secs(),
                        "WebSocket stuck in Connecting; resetting stale state and reconnecting"
                    );
                }
                WsState::Disconnected | WsState::Closing => {}
            }
            *state = WsState::Connecting;
            *self.connecting_since.lock() = Some(Instant::now());
        }
        tracing::info!(url = %self.url, "WebSocket connecting");

        let connect_result = match self.auth_token.as_deref() {
            Some(token) if !token.is_empty() => {
                use tokio_tungstenite::tungstenite::client::IntoClientRequest;
                match self.url.as_str().into_client_request() {
                    Ok(mut request) => {
                        match tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!(
                            "Bearer {token}"
                        )) {
                            Ok(value) => {
                                request.headers_mut().insert(
                                    tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
                                    value,
                                );
                            }
                            Err(e) => {
                                *self.state.write().await = WsState::Disconnected;
                                *self.connecting_since.lock() = None;
                                return Err(anyhow::anyhow!(
                                    "invalid remote auth token for Authorization header: {e}"
                                ));
                            }
                        }
                        if let Some(secret) = self.signing_secret.as_deref() {
                            let (ts, sig) = sign_timestamp(secret);
                            if let (Ok(ts_v), Ok(sig_v)) = (
                                tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&ts),
                                tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&sig),
                            ) {
                                request.headers_mut().insert("x-sen-timestamp", ts_v);
                                request.headers_mut().insert("x-sen-signature", sig_v);
                            }
                        }
                        tokio_tungstenite::connect_async(request).await
                    }
                    Err(e) => {
                        *self.state.write().await = WsState::Disconnected;
                        *self.connecting_since.lock() = None;
                        return Err(anyhow::anyhow!("invalid remote WebSocket URL: {e}"));
                    }
                }
            }
            _ => tokio_tungstenite::connect_async(&self.url).await,
        };
        let ws_stream = match connect_result {
            Ok((stream, _)) => stream,
            Err(e) => {
                *self.state.write().await = WsState::Disconnected;
                *self.connecting_since.lock() = None;
                return Err(anyhow::anyhow!("WebSocket connect failed: {e}"));
            }
        };

        let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();

        self.abort_io_tasks();
        self.shutdown.store(false, Ordering::SeqCst);
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let message_tx = self.message_tx.clone();
        let shutdown_read = Arc::clone(&self.shutdown);
        let state_read = Arc::clone(&self.state);
        let generation_read = Arc::clone(&self.generation);
        let connecting_read = Arc::clone(&self.connecting_since);
        let url_read = self.url.clone();
        let reader = crate::runtime::spawn_supervised("remote.websocket.reader", async move {
            while let Some(msg) = ws_stream_rx.next().await {
                if shutdown_read.load(Ordering::SeqCst) {
                    break;
                }
                match msg {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                        match serde_json::from_str::<WsMessage>(&text) {
                            Ok(parsed) => {
                                let _ = message_tx.send(parsed);
                                let queued = message_tx.len();
                                if queued
                                    >= REMOTE_BROADCAST_CAPACITY
                                        .saturating_sub(REMOTE_BROADCAST_CAPACITY / 8)
                                {
                                    tracing::warn!(
                                        url = %url_read,
                                        queued,
                                        capacity = REMOTE_BROADCAST_CAPACITY,
                                        "remote websocket broadcast nearing capacity; subscribers lagging"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "WebSocket JSON parse failed");
                            }
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                        tracing::info!("WebSocket closed by server");
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "WebSocket read error");
                        break;
                    }
                }
            }
            shutdown_read.store(true, Ordering::SeqCst);
            mark_disconnected_if_current(
                &state_read,
                &connecting_read,
                &generation_read,
                generation,
            )
            .await;
        });

        let out_rx = Arc::clone(&self.out_rx);
        let shutdown_write = Arc::clone(&self.shutdown);
        let state_write = Arc::clone(&self.state);
        let generation_write = Arc::clone(&self.generation);
        let connecting_write = Arc::clone(&self.connecting_since);
        let writer = crate::runtime::spawn_supervised("remote.websocket.writer", async move {
            let mut rx = out_rx.lock().await;
            while let Some(payload) = rx.recv().await {
                if shutdown_write.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(e) = ws_sink
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        payload.into(),
                    ))
                    .await
                {
                    tracing::warn!(error = %e, "WebSocket write error");
                    break;
                }
            }
            let _ = ws_sink
                .send(tokio_tungstenite::tungstenite::Message::Close(None))
                .await;
            shutdown_write.store(true, Ordering::SeqCst);
            mark_disconnected_if_current(
                &state_write,
                &connecting_write,
                &generation_write,
                generation,
            )
            .await;
        });

        *self.reader_task.lock() = Some(reader);
        *self.writer_task.lock() = Some(writer);

        *self.state.write().await = WsState::Connected;
        *self.connecting_since.lock() = None;
        Ok(())
    }

    pub async fn connect_with_retry(&self, max_attempts: u32) -> anyhow::Result<()> {
        let attempts = max_attempts.max(1);
        let mut delay = RETRY_INITIAL_DELAY;
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 1..=attempts {
            match self.connect().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!(
                        url = %self.url,
                        attempt,
                        max_attempts = attempts,
                        error = %e,
                        "WebSocket connect attempt failed"
                    );
                    last_err = Some(e);
                }
            }
            if attempt < attempts {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(RETRY_MAX_DELAY);
            }
        }
        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("WebSocket connect failed after {attempts} attempts")
        }))
    }

    pub fn start_supervised_connection(self: &Arc<Self>) {
        if let Some(handle) = self.supervisor_task.lock().take() {
            handle.abort();
        }
        self.closed.store(false, Ordering::SeqCst);
        let this = Arc::clone(self);
        let handle = crate::runtime::spawn_supervised("remote.websocket.supervisor", async move {
            let mut backoff = RETRY_INITIAL_DELAY;
            loop {
                if this.closed.load(Ordering::SeqCst) {
                    break;
                }
                let state = *this.state.read().await;
                match state {
                    WsState::Connected | WsState::Connecting | WsState::Closing => {
                        backoff = RETRY_INITIAL_DELAY;
                        tokio::time::sleep(WATCHDOG_POLL_INTERVAL).await;
                    }
                    WsState::Disconnected => {
                        if this.closed.load(Ordering::SeqCst) {
                            break;
                        }
                        this.fail_inflight_outbox();
                        match this.connect().await {
                            Ok(()) => {
                                backoff = RETRY_INITIAL_DELAY;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    url = %this.url,
                                    error = %e,
                                    backoff_secs = backoff.as_secs(),
                                    "remote websocket reconnect failed; backing off"
                                );
                                tokio::time::sleep(backoff).await;
                                backoff = (backoff * 2).min(RETRY_MAX_DELAY);
                            }
                        }
                    }
                }
            }
            tracing::info!(url = %this.url, "remote websocket supervisor stopped");
        });
        *self.supervisor_task.lock() = Some(handle);
    }

    fn fail_inflight_outbox(&self) {
        if let Ok(mut rx) = self.out_rx.try_lock() {
            let mut failed: u64 = 0;
            while rx.try_recv().is_ok() {
                failed += 1;
            }
            if failed > 0 {
                tracing::warn!(
                    url = %self.url,
                    failed,
                    "remote websocket reconnect: dropped in-flight outgoing messages"
                );
            }
        }
    }

    pub async fn disconnect(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(handle) = self.supervisor_task.lock().take() {
            handle.abort();
        }
        *self.state.write().await = WsState::Closing;
        self.shutdown.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.abort_io_tasks();
        tracing::info!(url = %self.url, "WebSocket disconnecting");
        *self.state.write().await = WsState::Disconnected;
        *self.connecting_since.lock() = None;
    }

    pub async fn send(&self, message: WsMessage) -> anyhow::Result<()> {
        let state = *self.state.read().await;
        if state != WsState::Connected {
            anyhow::bail!("WebSocket not connected (state: {:?})", state);
        }

        let payload = serde_json::to_string(&message)?;
        self.outbox
            .send(payload)
            .await
            .map_err(|_| anyhow::anyhow!("WebSocket outbox closed"))?;
        tracing::debug!(msg_type = %message.msg_type, "WebSocket sending");
        Ok(())
    }
}

async fn mark_disconnected_if_current(
    state: &Arc<RwLock<WsState>>,
    connecting_since: &Arc<parking_lot::Mutex<Option<Instant>>>,
    generation: &Arc<AtomicU64>,
    my_generation: u64,
) {
    if generation.load(Ordering::SeqCst) != my_generation {
        return;
    }
    let mut st = state.write().await;
    if generation.load(Ordering::SeqCst) != my_generation {
        return;
    }
    if matches!(*st, WsState::Connected | WsState::Connecting) {
        *st = WsState::Disconnected;
        *connecting_since.lock() = None;
    }
}
