// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use regex::Regex;
use uuid::Uuid;

fn ensure_https(url: &str) -> anyhow::Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!(
            "Refusing to transmit sensitive data over non-HTTPS URL: URL scheme must be https"
        );
    }
    Ok(())
}

pub struct WhatsAppChannel {
    access_token: String,
    endpoint_id: String,
    verify_token: String,
    allowed_numbers: Vec<String>,

    proxy_url: Option<String>,

    dm_mention_patterns: Vec<Regex>,

    group_mention_patterns: Vec<Regex>,
}

impl WhatsAppChannel {
    pub fn new(
        access_token: String,
        endpoint_id: String,
        verify_token: String,
        allowed_numbers: Vec<String>,
    ) -> Self {
        Self {
            access_token,
            endpoint_id,
            verify_token,
            allowed_numbers,
            proxy_url: None,
            dm_mention_patterns: Vec::new(),
            group_mention_patterns: Vec::new(),
        }
    }

    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    pub fn with_dm_mention_patterns(mut self, patterns: Vec<String>) -> Self {
        self.dm_mention_patterns = Self::compile_mention_patterns(&patterns);
        self
    }

    pub fn with_group_mention_patterns(mut self, patterns: Vec<String>) -> Self {
        self.group_mention_patterns = Self::compile_mention_patterns(&patterns);
        self
    }

    pub(crate) fn compile_mention_patterns(patterns: &[String]) -> Vec<Regex> {
        patterns
            .iter()
            .filter_map(|p| {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    return None;
                }
                match regex::RegexBuilder::new(trimmed)
                    .case_insensitive(true)
                    .size_limit(1 << 16)
                    .build()
                {
                    Ok(re) => Some(re),
                    Err(e) => {
                        tracing::warn!(
                            "WhatsApp: ignoring invalid mention_pattern {trimmed:?}: {e}"
                        );
                        None
                    }
                }
            })
            .collect()
    }

    pub(crate) fn text_matches_patterns(patterns: &[Regex], text: &str) -> bool {
        patterns.iter().any(|re| re.is_match(text))
    }

    pub(crate) fn strip_patterns(patterns: &[Regex], text: &str) -> Option<String> {
        let mut result = text.to_string();
        for re in patterns {
            result = re.replace_all(&result, " ").into_owned();
        }
        let normalized = result.split_whitespace().collect::<Vec<_>>().join(" ");
        (!normalized.is_empty()).then_some(normalized)
    }

    pub(crate) fn apply_mention_gating(
        dm_patterns: &[Regex],
        group_patterns: &[Regex],
        content: &str,
        is_group: bool,
    ) -> Option<String> {
        let patterns = if is_group {
            group_patterns
        } else {
            dm_patterns
        };
        if patterns.is_empty() {
            return Some(content.to_string());
        }
        if !Self::text_matches_patterns(patterns, content) {
            return None;
        }
        Self::strip_patterns(patterns, content)
    }

    fn is_group_message(msg: &serde_json::Value) -> bool {
        msg.get("context")
            .and_then(|ctx| ctx.get("group_id"))
            .and_then(|g| g.as_str())
            .is_some_and(|s| !s.is_empty())
    }

    fn http_client(&self) -> reqwest::Client {
        crate::config::build_channel_proxy_client("channel.whatsapp", self.proxy_url.as_deref())
    }

    fn is_number_allowed(&self, phone: &str) -> bool {
        self.allowed_numbers.iter().any(|n| n == "*" || n == phone)
    }

    pub fn verify_token(&self) -> &str {
        &self.verify_token
    }

    pub fn parse_webhook_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let Some(entries) = payload.get("entry").and_then(|e| e.as_array()) else {
            return messages;
        };

        for entry in entries {
            let Some(changes) = entry.get("changes").and_then(|c| c.as_array()) else {
                continue;
            };

            for change in changes {
                let Some(value) = change.get("value") else {
                    continue;
                };

                let Some(msgs) = value.get("messages").and_then(|m| m.as_array()) else {
                    continue;
                };

                for msg in msgs {

                    let Some(from) = msg.get("from").and_then(|f| f.as_str()) else {
                        continue;
                    };

                    let normalized_from = if from.starts_with('+') {
                        from.to_string()
                    } else {
                        format!("+{from}")
                    };

                    if !self.is_number_allowed(&normalized_from) {
                        tracing::warn!(
                            "WhatsApp: ignoring message from unauthorized number: {normalized_from}. \
                            Add to channels.whatsapp.allowed_numbers in config.toml, \
                            or run `sen onboard --channels-only` to configure interactively."
                        );
                        continue;
                    }

                    let content = if let Some(text_obj) = msg.get("text") {
                        text_obj
                            .get("body")
                            .and_then(|b| b.as_str())
                            .unwrap_or("")
                            .to_string()
                    } else {

                        tracing::debug!("WhatsApp: skipping non-text message from {from}");
                        continue;
                    };

                    if content.is_empty() {
                        continue;
                    }

                    let is_group = Self::is_group_message(msg);
                    let content = match Self::apply_mention_gating(
                        &self.dm_mention_patterns,
                        &self.group_mention_patterns,
                        &content,
                        is_group,
                    ) {
                        Some(c) => c,
                        None => {
                            tracing::debug!(
                                "WhatsApp: message from {from} did not match mention patterns, dropping"
                            );
                            continue;
                        }
                    };

                    let timestamp = msg
                        .get("timestamp")
                        .and_then(|t| t.as_str())
                        .and_then(|t| t.parse::<u64>().ok())
                        .unwrap_or_else(|| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        });

                    messages.push(ChannelMessage {
                        id: Uuid::new_v4().to_string(),
                        reply_target: normalized_from.clone(),
                        sender: normalized_from,
                        content,
                        channel: "whatsapp".to_string(),
                        timestamp,
                        thread_ts: None,
                        interruption_scope_id: None,
                        attachments: vec![],
                    });
                }
            }
        }

        messages
    }
}

#[async_trait]
impl Channel for WhatsAppChannel {
    fn name(&self) -> &str {
        "whatsapp"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        let url = format!(
            "https://graph.facebook.com/v18.0/{}/messages",
            self.endpoint_id
        );

        let to = message
            .recipient
            .strip_prefix('+')
            .unwrap_or(&message.recipient);

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": {
                "preview_url": false,
                "body": message.content
            }
        });

        ensure_https(&url)?;

        let resp = self
            .http_client()
            .post(&url)
            .bearer_auth(&self.access_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_default();
            tracing::error!("WhatsApp send failed: {status} — {error_body}");
            anyhow::bail!("WhatsApp API error: {status}");
        }

        Ok(())
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {

        tracing::info!(
            "WhatsApp channel active (webhook mode). \
            Configure Meta webhook to POST to your gateway's /whatsapp endpoint."
        );

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    }

    async fn health_check(&self) -> bool {

        let url = format!("https://graph.facebook.com/v18.0/{}", self.endpoint_id);

        if ensure_https(&url).is_err() {
            return false;
        }

        self.http_client()
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
