// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use uuid::Uuid;

pub struct LinqChannel {
    api_token: String,
    from_phone: String,
    allowed_senders: Vec<String>,
    client: reqwest::Client,
}

const LINQ_API_BASE: &str = "https://api.linqapp.com/api/partner/v3";

impl LinqChannel {
    pub fn new(api_token: String, from_phone: String, allowed_senders: Vec<String>) -> Self {
        Self {
            api_token,
            from_phone,
            allowed_senders,
            client: reqwest::Client::new(),
        }
    }

    fn is_sender_allowed(&self, phone: &str) -> bool {
        self.allowed_senders.iter().any(|n| n == "*" || n == phone)
    }

    pub fn phone_number(&self) -> &str {
        &self.from_phone
    }

    fn media_part_to_image_marker(part: &serde_json::Value) -> Option<String> {
        let source = part
            .get("url")
            .or_else(|| part.get("value"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())?;

        let mime_type = part
            .get("mime_type")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_ascii_lowercase();

        if !mime_type.starts_with("image/") {
            return None;
        }

        Some(format!("[IMAGE:{source}]"))
    }

    fn sender_is_from_me(data: &serde_json::Value) -> bool {

        if let Some(v) = data.get("is_from_me").and_then(|value| value.as_bool()) {
            return v;
        }

        let is_me = data
            .get("sender_handle")
            .and_then(|value| value.get("is_me"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let is_outbound = matches!(
            data.get("direction").and_then(|value| value.as_str()),
            Some("outbound")
        );

        is_me || is_outbound
    }

    fn sender_handle(data: &serde_json::Value) -> Option<&str> {
        data.get("from")
            .and_then(|value| value.as_str())
            .or_else(|| {
                data.get("sender_handle")
                    .and_then(|value| value.get("handle"))
                    .and_then(|value| value.as_str())
            })
    }

    fn chat_id(data: &serde_json::Value) -> Option<&str> {
        data.get("chat_id")
            .and_then(|value| value.as_str())
            .or_else(|| {
                data.get("chat")
                    .and_then(|value| value.get("id"))
                    .and_then(|value| value.as_str())
            })
    }

    fn message_parts(data: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
        data.get("message")
            .and_then(|value| value.get("parts"))
            .and_then(|value| value.as_array())
            .or_else(|| data.get("parts").and_then(|value| value.as_array()))
    }

    pub fn parse_webhook_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let event_type = payload
            .get("event_type")
            .and_then(|e| e.as_str())
            .unwrap_or("");
        if event_type != "message.received" {
            tracing::debug!("Linq: skipping non-message event: {event_type}");
            return messages;
        }

        let Some(data) = payload.get("data") else {
            return messages;
        };

        if Self::sender_is_from_me(data) {
            tracing::debug!("Linq: skipping is_from_me message");
            return messages;
        }

        let Some(from) = Self::sender_handle(data) else {
            return messages;
        };

        let normalized_from = if from.starts_with('+') {
            from.to_string()
        } else {
            format!("+{from}")
        };

        if !self.is_sender_allowed(&normalized_from) {
            tracing::warn!(
                "Linq: ignoring message from unauthorized sender: {normalized_from}. \
                Add to channels.linq.allowed_senders in config.toml, \
                or run `sen onboard --channels-only` to configure interactively."
            );
            return messages;
        }

        let chat_id = Self::chat_id(data).unwrap_or("").to_string();

        let Some(parts) = Self::message_parts(data) else {
            return messages;
        };

        let content_parts: Vec<String> = parts
            .iter()
            .filter_map(|part| {
                let part_type = part.get("type").and_then(|t| t.as_str())?;
                match part_type {
                    "text" => part
                        .get("value")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string),
                    "media" | "image" => {
                        if let Some(marker) = Self::media_part_to_image_marker(part) {
                            Some(marker)
                        } else {
                            tracing::debug!("Linq: skipping unsupported {part_type} part");
                            None
                        }
                    }
                    _ => {
                        tracing::debug!("Linq: skipping {part_type} part");
                        None
                    }
                }
            })
            .collect();

        if content_parts.is_empty() {
            return messages;
        }

        let content = content_parts.join("\n").trim().to_string();

        if content.is_empty() {
            return messages;
        }

        let timestamp = payload
            .get("created_at")
            .and_then(|t| t.as_str())
            .and_then(|t| {
                chrono::DateTime::parse_from_rfc3339(t)
                    .ok()
                    .map(|dt| dt.timestamp().cast_unsigned())
            })
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            });

        let reply_target = if chat_id.is_empty() {
            normalized_from.clone()
        } else {
            chat_id
        };

        messages.push(ChannelMessage {
            id: Uuid::new_v4().to_string(),
            reply_target,
            sender: normalized_from,
            content,
            channel: "linq".to_string(),
            timestamp,
            thread_ts: None,
            interruption_scope_id: None,
            attachments: vec![],
        });

        messages
    }
}

#[async_trait]
impl Channel for LinqChannel {
    fn name(&self) -> &str {
        "linq"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        let recipient = &message.recipient;

        let body = serde_json::json!({
            "message": {
                "parts": [{
                    "type": "text",
                    "value": message.content
                }]
            }
        });

        let url = format!("{LINQ_API_BASE}/chats/{recipient}/messages");

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            return Ok(());
        }

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            let new_chat_body = serde_json::json!({
                "from": self.from_phone,
                "to": [recipient],
                "message": {
                    "parts": [{
                        "type": "text",
                        "value": message.content
                    }]
                }
            });

            let create_resp = self
                .client
                .post(format!("{LINQ_API_BASE}/chats"))
                .bearer_auth(&self.api_token)
                .header("Content-Type", "application/json")
                .json(&new_chat_body)
                .send()
                .await?;

            if !create_resp.status().is_success() {
                let status = create_resp.status();
                let error_body = create_resp.text().await.unwrap_or_default();
                tracing::error!("Linq create chat failed: {status} — {error_body}");
                anyhow::bail!("Linq API error: {status}");
            }

            return Ok(());
        }

        let status = resp.status();
        let error_body = resp.text().await.unwrap_or_default();
        tracing::error!("Linq send failed: {status} — {error_body}");
        anyhow::bail!("Linq API error: {status}");
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {

        tracing::info!(
            "Linq channel active (webhook mode). \
            Configure Linq webhook to POST to your gateway's /linq endpoint."
        );

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    }

    async fn health_check(&self) -> bool {

        let url = format!("{LINQ_API_BASE}/phonenumbers");

        self.client
            .get(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let url = format!("{LINQ_API_BASE}/chats/{recipient}/typing");

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            tracing::debug!("Linq start_typing failed: {}", resp.status());
        }

        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let url = format!("{LINQ_API_BASE}/chats/{recipient}/typing");

        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            tracing::debug!("Linq stop_typing failed: {}", resp.status());
        }

        Ok(())
    }
}

pub fn verify_linq_signature(secret: &str, body: &str, timestamp: &str, signature: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    if let Ok(ts) = timestamp.parse::<i64>() {
        let now = chrono::Utc::now().timestamp();
        if (now - ts).unsigned_abs() > 300 {
            tracing::warn!("Linq: rejecting stale webhook timestamp ({ts}, now={now})");
            return false;
        }
    } else {
        tracing::warn!("Linq: invalid webhook timestamp: {timestamp}");
        return false;
    }

    let message = format!("{timestamp}.{body}");
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(message.as_bytes());
    let signature_hex = signature
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(signature);
    let Ok(provided) = hex::decode(signature_hex.trim()) else {
        tracing::warn!("Linq: invalid webhook signature format");
        return false;
    };

    mac.verify_slice(&provided).is_ok()
}
