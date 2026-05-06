// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct WebhookChannel {
    listen_port: u16,
    listen_path: String,
    send_url: Option<String>,
    send_method: String,
    auth_header: Option<String>,
    secret: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IncomingWebhook {
    sender: String,
    content: String,
    #[serde(default)]
    thread_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OutgoingWebhook {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recipient: Option<String>,
}

impl WebhookChannel {
    pub fn new(
        listen_port: u16,
        listen_path: Option<String>,
        send_url: Option<String>,
        send_method: Option<String>,
        auth_header: Option<String>,
        secret: Option<String>,
    ) -> Self {
        let path = listen_path.unwrap_or_else(|| "/webhook".to_string());

        let listen_path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };

        Self {
            listen_port,
            listen_path,
            send_url,
            send_method: send_method
                .unwrap_or_else(|| "POST".to_string())
                .to_uppercase(),
            auth_header,
            secret,
        }
    }

    fn http_client(&self) -> reqwest::Client {
        crate::config::build_runtime_proxy_client("channel.webhook")
    }

    fn verify_signature(&self, body: &[u8], signature: Option<&str>) -> bool {
        let Some(ref secret) = self.secret else {
            return true;
        };

        let Some(sig) = signature else {
            return false;
        };

        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;

        let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(body);

        let Ok(expected) = hex::decode(sig.trim_start_matches("sha256=")) else {
            return false;
        };

        mac.verify_slice(&expected).is_ok()
    }
}

#[async_trait]
impl Channel for WebhookChannel {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {
        let Some(ref send_url) = self.send_url else {
            tracing::debug!("Webhook channel: no send_url configured, skipping outbound message");
            return Ok(());
        };

        let client = self.http_client();
        let payload = OutgoingWebhook {
            content: message.content.clone(),
            thread_id: message.thread_ts.clone(),
            recipient: if message.recipient.is_empty() {
                None
            } else {
                Some(message.recipient.clone())
            },
        };

        let mut request = match self.send_method.as_str() {
            "PUT" => client.put(send_url),
            _ => client.post(send_url),
        };

        if let Some(ref auth) = self.auth_header {
            request = request.header("Authorization", auth);
        }

        let resp = request.json(&payload).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response: {e}>"));
            bail!("Webhook send failed ({status}): {body}");
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<()> {
        use axum::{
            Router,
            body::Bytes,
            extract::State,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        use portable_atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        let counter = Arc::new(AtomicU64::new(0));

        struct WebhookState {
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
            secret: Option<String>,
            counter: Arc<AtomicU64>,
        }

        let state = Arc::new(WebhookState {
            tx: tx.clone(),
            secret: self.secret.clone(),
            counter: counter.clone(),
        });

        let listen_path = self.listen_path.clone();

        async fn handle_webhook(
            State(state): State<Arc<WebhookState>>,
            headers: HeaderMap,
            body: Bytes,
        ) -> StatusCode {

            if let Some(ref secret) = state.secret {
                use hmac::{Hmac, Mac};
                use sha2::Sha256;
                type HmacSha256 = Hmac<Sha256>;

                let signature = headers
                    .get("x-webhook-signature")
                    .and_then(|v| v.to_str().ok());

                let valid = if let Some(sig) = signature {
                    if let Ok(decoded) = hex::decode(sig.trim_start_matches("sha256=")) {
                        if let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) {
                            mac.update(&body);
                            mac.verify_slice(&decoded).is_ok()
                        } else {
                            tracing::warn!("Webhook: failed to create HMAC");
                            false
                        }
                    } else {

                        tracing::warn!(
                            "Webhook: invalid hex signature (not base16), rejecting request"
                        );
                        false
                    }
                } else {
                    false
                };

                if !valid {
                    tracing::warn!("Webhook: invalid signature, rejecting request");
                    return StatusCode::UNAUTHORIZED;
                }
            }

            let payload: IncomingWebhook = match serde_json::from_slice(&body) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Webhook: invalid JSON payload: {e}");
                    return StatusCode::BAD_REQUEST;
                }
            };

            if payload.content.is_empty() {
                return StatusCode::BAD_REQUEST;
            }

            let seq = state.counter.fetch_add(1, Ordering::Relaxed);

            #[allow(clippy::cast_possible_truncation)]
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let reply_target = payload
                .thread_id
                .clone()
                .unwrap_or_else(|| payload.sender.clone());

            let msg = ChannelMessage {
                id: format!("webhook_{seq}"),
                sender: payload.sender,
                reply_target,
                content: payload.content,
                channel: "webhook".to_string(),
                timestamp,
                thread_ts: payload.thread_id,
                interruption_scope_id: None,
                attachments: vec![],
            };

            if state.tx.send(msg).await.is_err() {
                return StatusCode::SERVICE_UNAVAILABLE;
            }

            StatusCode::OK
        }

        let app = Router::new()
            .route(&listen_path, post(handle_webhook))
            .with_state(state);

        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], self.listen_port));
        tracing::info!(
            "Webhook channel listening on http://0.0.0.0:{}{} ...",
            self.listen_port,
            self.listen_path
        );

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .await
            .map_err(|e| anyhow::anyhow!("Webhook server error: {e}"))?;

        Ok(())
    }

    async fn health_check(&self) -> bool {

        true
    }
}
