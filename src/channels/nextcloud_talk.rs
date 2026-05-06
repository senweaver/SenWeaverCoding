// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

pub struct NextcloudTalkChannel {
    base_url: String,
    app_token: String,
    bot_name: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl NextcloudTalkChannel {
    pub fn new(
        base_url: String,
        app_token: String,
        bot_name: String,
        allowed_users: Vec<String>,
    ) -> Self {
        Self::new_with_proxy(base_url, app_token, bot_name, allowed_users, None)
    }

    pub fn new_with_proxy(
        base_url: String,
        app_token: String,
        bot_name: String,
        allowed_users: Vec<String>,
        proxy_url: Option<String>,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            app_token,
            bot_name: bot_name.to_ascii_lowercase(),
            allowed_users,
            client: crate::config::build_channel_proxy_client(
                "channel.nextcloud_talk",
                proxy_url.as_deref(),
            ),
        }
    }

    fn is_user_allowed(&self, actor_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == actor_id)
    }

    fn is_bot_name(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();

        (!self.bot_name.is_empty() && name == self.bot_name) || name == "sen"
    }

    fn now_unix_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn parse_timestamp_secs(value: Option<&serde_json::Value>) -> u64 {
        let raw = match value {
            Some(serde_json::Value::Number(num)) => num.as_u64(),
            Some(serde_json::Value::String(s)) => s.trim().parse::<u64>().ok(),
            _ => None,
        }
        .unwrap_or_else(Self::now_unix_secs);

        if raw > 1_000_000_000_000 {
            raw / 1000
        } else {
            raw
        }
    }

    fn value_to_string(value: Option<&serde_json::Value>) -> Option<String> {
        match value {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    }

    pub fn parse_webhook_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let messages = Vec::new();

        let event_type = match payload.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return messages,
        };

        if event_type.eq_ignore_ascii_case("create") {
            return self.parse_as2_payload(payload);
        }

        if !event_type.eq_ignore_ascii_case("message") {
            tracing::debug!("Nextcloud Talk: skipping non-message event: {event_type}");
            return messages;
        }

        self.parse_message_payload(payload)
    }

    fn parse_as2_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let obj = match payload.get("object") {
            Some(o) => o,
            None => return messages,
        };

        let object_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if !object_type.eq_ignore_ascii_case("note") {
            tracing::debug!("Nextcloud Talk: skipping AS2 Create with object.type={object_type}");
            return messages;
        }

        let room_token = payload
            .get("target")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty());

        let Some(room_token) = room_token else {
            tracing::warn!("Nextcloud Talk: missing target.id (room token) in AS2 payload");
            return messages;
        };

        let actor = payload.get("actor").cloned().unwrap_or_default();
        let actor_type = actor.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if actor_type.eq_ignore_ascii_case("application") {
            tracing::debug!(
                "Nextcloud Talk: skipping bot-originated AS2 message (type=Application)"
            );
            return messages;
        }

        let actor_id = actor
            .get("id")
            .and_then(|v| v.as_str())
            .map(|id| {
                id.trim_start_matches("users/")
                    .trim_start_matches("bots/")
                    .trim()
            })
            .filter(|id| !id.is_empty());

        let Some(actor_id) = actor_id else {
            tracing::warn!("Nextcloud Talk: missing actor.id in AS2 payload");
            return messages;
        };

        let raw_actor_id = actor.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if raw_actor_id.starts_with("bots/") {
            tracing::debug!(
                "Nextcloud Talk: skipping bot-originated AS2 message (id prefix=bots/)"
            );
            return messages;
        }
        let actor_name = actor
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if self.is_bot_name(&actor_name) {
            tracing::debug!(
                "Nextcloud Talk: skipping bot-originated AS2 message (name={actor_name})"
            );
            return messages;
        }

        if !self.is_user_allowed(actor_id) {
            tracing::warn!(
                "Nextcloud Talk: ignoring message from unauthorized actor: {actor_id}. \
                Add to channels.nextcloud_talk.allowed_users in config.toml, \
                or run `sen onboard --channels-only` to configure interactively."
            );
            return messages;
        }

        let content = obj
            .get("content")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|v| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::trim)
                    .map(str::to_string)
            })
            .filter(|s| !s.is_empty());

        let Some(content) = content else {
            tracing::debug!("Nextcloud Talk: empty or unparseable AS2 message content");
            return messages;
        };

        let message_id =
            Self::value_to_string(obj.get("id")).unwrap_or_else(|| Uuid::new_v4().to_string());

        messages.push(ChannelMessage {
            id: message_id,
            reply_target: room_token.to_string(),
            sender: actor_id.to_string(),
            content,
            channel: "nextcloud_talk".to_string(),
            timestamp: Self::now_unix_secs(),
            thread_ts: None,
            interruption_scope_id: None,
            attachments: vec![],
        });

        messages
    }

    fn parse_message_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let Some(message_obj) = payload.get("message") else {
            return messages;
        };

        let room_token = payload
            .get("object")
            .and_then(|obj| obj.get("token"))
            .and_then(|v| v.as_str())
            .or_else(|| message_obj.get("token").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|token| !token.is_empty());

        let Some(room_token) = room_token else {
            tracing::warn!("Nextcloud Talk: missing room token in webhook payload");
            return messages;
        };

        let actor_type = message_obj
            .get("actorType")
            .and_then(|v| v.as_str())
            .or_else(|| payload.get("actorType").and_then(|v| v.as_str()))
            .unwrap_or("");

        if actor_type.eq_ignore_ascii_case("bots") || actor_type.eq_ignore_ascii_case("application")
        {
            tracing::debug!(
                "Nextcloud Talk: skipping bot-originated message (actorType={actor_type})"
            );
            return messages;
        }

        let actor_id = message_obj
            .get("actorId")
            .and_then(|v| v.as_str())
            .or_else(|| payload.get("actorId").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|id| !id.is_empty());

        let Some(actor_id) = actor_id else {
            tracing::warn!("Nextcloud Talk: missing actorId in webhook payload");
            return messages;
        };

        if self.is_bot_name(actor_id) {
            tracing::debug!("Nextcloud Talk: skipping bot-originated message (actorId={actor_id})");
            return messages;
        }

        if !self.is_user_allowed(actor_id) {
            tracing::warn!(
                "Nextcloud Talk: ignoring message from unauthorized actor: {actor_id}. \
                Add to channels.nextcloud_talk.allowed_users in config.toml, \
                or run `sen onboard --channels-only` to configure interactively."
            );
            return messages;
        }

        let message_type = message_obj
            .get("messageType")
            .and_then(|v| v.as_str())
            .unwrap_or("comment");
        if !message_type.eq_ignore_ascii_case("comment") {
            tracing::debug!("Nextcloud Talk: skipping non-comment messageType: {message_type}");
            return messages;
        }

        let has_system_message = message_obj
            .get("systemMessage")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if has_system_message {
            tracing::debug!("Nextcloud Talk: skipping system message event");
            return messages;
        }

        let content = message_obj
            .get("message")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|content| !content.is_empty());

        let Some(content) = content else {
            return messages;
        };

        let message_id = Self::value_to_string(message_obj.get("id"))
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let timestamp = Self::parse_timestamp_secs(message_obj.get("timestamp"));

        messages.push(ChannelMessage {
            id: message_id,
            reply_target: room_token.to_string(),
            sender: actor_id.to_string(),
            content: content.to_string(),
            channel: "nextcloud_talk".to_string(),
            timestamp,
            thread_ts: None,
            interruption_scope_id: None,
            attachments: vec![],
        });

        messages
    }

    async fn send_to_room(&self, room_token: &str, content: &str) -> anyhow::Result<()> {
        let encoded_room = urlencoding::encode(room_token);
        let url = format!(
            "{}/ocs/v2.php/apps/spreed/api/v1/chat/{}?format=json",
            self.base_url, encoded_room
        );

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.app_token)
            .header("OCS-APIRequest", "true")
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "message": content }))
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("Nextcloud Talk send failed: {status} — {body}");
        anyhow::bail!("Nextcloud Talk API error: {status}");
    }
}

#[async_trait]
impl Channel for NextcloudTalkChannel {
    fn name(&self) -> &str {
        "nextcloud_talk"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        self.send_to_room(&message.recipient, &message.content)
            .await
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        tracing::info!(
            "Nextcloud Talk channel active (webhook mode). \
            Configure Nextcloud Talk bot webhook to POST to your gateway's /nextcloud-talk endpoint."
        );

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/status.php", self.base_url);

        self.client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

pub fn verify_nextcloud_talk_signature(
    secret: &str,
    random: &str,
    body: &str,
    signature: &str,
) -> bool {
    let random = random.trim();
    if random.is_empty() {
        tracing::warn!("Nextcloud Talk: missing X-Nextcloud-Talk-Random header");
        return false;
    }

    let signature_hex = signature
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(signature)
        .trim();

    let Ok(provided) = hex::decode(signature_hex) else {
        tracing::warn!("Nextcloud Talk: invalid signature format");
        return false;
    };

    let payload = format!("{random}{body}");
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(payload.as_bytes());

    mac.verify_slice(&provided).is_ok()
}
