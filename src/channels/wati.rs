// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use uuid::Uuid;

const MAX_WATI_AUDIO_BYTES: u64 = 25 * 1024 * 1024;

pub struct WatiChannel {
    api_token: String,
    api_url: String,
    tenant_id: Option<String>,
    allowed_numbers: Vec<String>,
    client: reqwest::Client,
    transcription_manager: Option<std::sync::Arc<super::transcription::TranscriptionManager>>,
}

impl WatiChannel {
    pub fn new(
        api_token: String,
        api_url: String,
        tenant_id: Option<String>,
        allowed_numbers: Vec<String>,
    ) -> Self {
        Self::new_with_proxy(api_token, api_url, tenant_id, allowed_numbers, None)
    }

    pub fn new_with_proxy(
        api_token: String,
        api_url: String,
        tenant_id: Option<String>,
        allowed_numbers: Vec<String>,
        proxy_url: Option<String>,
    ) -> Self {
        Self {
            api_token,
            api_url,
            tenant_id,
            allowed_numbers,
            client: crate::services::get_services()
                .proxy_runtime()
                .build_channel_client("channel.wati", proxy_url.as_deref()),
            transcription_manager: None,
        }
    }

    pub fn with_transcription(mut self, config: crate::config::TranscriptionConfig) -> Self {
        if !config.enabled {
            return self;
        }
        match super::transcription::TranscriptionManager::new(&config) {
            Ok(m) => {
                self.transcription_manager = Some(std::sync::Arc::new(m));
            }
            Err(e) => {
                tracing::warn!(
                    "transcription manager init failed, voice transcription disabled: {e}"
                );
            }
        }
        self
    }

    fn is_number_allowed(&self, phone: &str) -> bool {
        self.allowed_numbers.iter().any(|n| n == "*" || n == phone)
    }

    fn extract_sender(&self, payload: &serde_json::Value) -> Option<String> {

        let wa_id = payload
            .get("waId")
            .or_else(|| payload.get("wa_id"))
            .or_else(|| payload.get("from"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if wa_id.is_empty() {
            return None;
        }

        let normalized_phone = if wa_id.starts_with('+') {
            wa_id.to_string()
        } else {
            format!("+{wa_id}")
        };

        if !self.is_number_allowed(&normalized_phone) {
            tracing::warn!(
                "WATI: ignoring message from unauthorized sender: {normalized_phone}. \
                Add to channels.wati.allowed_numbers in config.toml, \
                or run `sen onboard --channels-only` to configure interactively."
            );
            return None;
        }

        Some(normalized_phone)
    }

    fn build_target(&self, phone: &str) -> String {

        let bare = phone.strip_prefix('+').unwrap_or(phone);
        if let Some(ref tid) = self.tenant_id {
            if bare.starts_with(&format!("{tid}:")) {
                bare.to_string()
            } else {
                format!("{tid}:{bare}")
            }
        } else {
            bare.to_string()
        }
    }

    fn extract_timestamp(payload: &serde_json::Value) -> u64 {
        payload
            .get("timestamp")
            .or_else(|| payload.get("created"))
            .map(|t| {
                if let Some(secs) = t.as_u64() {
                    if secs > 10_000_000_000 {
                        secs / 1000
                    } else {
                        secs
                    }
                } else if let Some(s) = t.as_str() {
                    chrono::DateTime::parse_from_rfc3339(s)
                        .ok()
                        .map(|dt| dt.timestamp().cast_unsigned())
                        .unwrap_or_else(|| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        })
                } else {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                }
            })
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
    }

    pub fn parse_webhook_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let text = payload
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| {
                payload
                    .get("message")
                    .and_then(|m| m.get("text").or_else(|| m.get("body")))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("")
            .trim();

        if text.is_empty() {
            return messages;
        }

        let from_me = payload
            .get("fromMe")
            .or_else(|| payload.get("from_me"))
            .or_else(|| payload.get("owner"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if from_me {
            tracing::debug!("WATI: skipping fromMe message");
            return messages;
        }

        let Some(normalized_phone) = self.extract_sender(payload) else {
            return messages;
        };

        let timestamp = Self::extract_timestamp(payload);
        messages.push(ChannelMessage {
            id: Uuid::new_v4().to_string(),
            reply_target: normalized_phone.clone(),
            sender: normalized_phone,
            content: text.to_string(),
            channel: "wati".to_string(),
            timestamp,
            thread_ts: None,
            interruption_scope_id: None,
            attachments: vec![],
        });

        messages
    }

    fn extract_host(url_str: &str) -> Option<String> {
        reqwest::Url::parse(url_str)
            .ok()?
            .host_str()
            .map(|h| h.to_ascii_lowercase())
    }

    pub async fn try_transcribe_audio(&self, payload: &serde_json::Value) -> Option<String> {
        let manager = self.transcription_manager.as_deref()?;

        let media_url = payload
            .get("mediaUrl")
            .or_else(|| payload.get("media_url"))
            .and_then(|v| v.as_str())?;

        let api_host = Self::extract_host(&self.api_url);
        let media_host = Self::extract_host(media_url);
        match (api_host, media_host) {
            (Some(ref expected), Some(ref actual)) if actual == expected => {}
            _ => {
                tracing::warn!("WATI: blocked media URL with unexpected host: {media_url}");
                return None;
            }
        }

        let from_me = payload
            .get("fromMe")
            .or_else(|| payload.get("from_me"))
            .or_else(|| payload.get("owner"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if from_me {
            tracing::debug!("WATI: skipping fromMe audio before download");
            return None;
        }

        let msg_type = payload
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("audio");

        let file_name = match msg_type {
            "voice" => "voice.ogg",
            _ => "audio.ogg",
        };

        let mut resp = match self
            .client
            .get(media_url)
            .bearer_auth(&self.api_token)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("WATI: media download request failed: {e}");
                return None;
            }
        };

        if !resp.status().is_success() {
            tracing::warn!("WATI: media download failed: {}", resp.status());
            return None;
        }

        let mut audio_bytes = Vec::new();
        while let Some(chunk) = resp.chunk().await.ok().flatten() {
            audio_bytes.extend_from_slice(&chunk);
            if audio_bytes.len() as u64 > MAX_WATI_AUDIO_BYTES {
                tracing::warn!(
                    "WATI: audio download exceeds {} byte limit",
                    MAX_WATI_AUDIO_BYTES
                );
                return None;
            }
        }

        match manager.transcribe(&audio_bytes, file_name).await {
            Ok(transcript) => Some(transcript),
            Err(e) => {
                tracing::warn!("WATI: transcription failed: {e}");
                None
            }
        }
    }

    pub fn parse_audio_as_message(
        &self,
        payload: &serde_json::Value,
        transcript: String,
    ) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let from_me = payload
            .get("fromMe")
            .or_else(|| payload.get("from_me"))
            .or_else(|| payload.get("owner"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if from_me {
            tracing::debug!("WATI: skipping fromMe audio message");
            return messages;
        }

        if transcript.trim().is_empty() {
            tracing::debug!("WATI: skipping empty audio transcript");
            return messages;
        }

        let Some(normalized_phone) = self.extract_sender(payload) else {
            return messages;
        };

        let timestamp = Self::extract_timestamp(payload);
        messages.push(ChannelMessage {
            id: Uuid::new_v4().to_string(),
            reply_target: normalized_phone.clone(),
            sender: normalized_phone,
            content: transcript,
            channel: "wati".to_string(),
            timestamp,
            thread_ts: None,
            interruption_scope_id: None,
            attachments: vec![],
        });

        messages
    }
}

#[async_trait]
impl Channel for WatiChannel {
    fn name(&self) -> &str {
        "wati"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let target = self.build_target(&message.recipient);

        let body = serde_json::json!({
            "target": target,
            "text": message.content
        });

        let url = format!("{}/api/ext/v3/conversations/messages/text", self.api_url);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_default();
            tracing::error!("WATI send failed: {status} — {error_body}");
            anyhow::bail!("WATI API error: {status}");
        }

        Ok(())
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {

        tracing::info!(
            "WATI channel active (webhook mode). \
            Configure WATI webhook to POST to your gateway's /wati endpoint."
        );

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/api/ext/v3/contacts/count", self.api_url);

        self.client
            .get(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn start_typing(&self, _recipient: &str) -> anyhow::Result<()> {

        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {

        Ok(())
    }
}
