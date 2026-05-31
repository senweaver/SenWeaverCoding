// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info, warn};

use super::traits::{Channel, ChannelMessage, SendMessage};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GmailPushConfig {

    #[serde(default)]
    pub enabled: bool,

    pub topic: String,

    #[serde(default = "default_label_filter")]
    pub label_filter: Vec<String>,

    #[serde(default)]
    pub oauth_token: String,

    #[serde(default)]
    pub allowed_senders: Vec<String>,

    #[serde(default)]
    pub webhook_url: String,

    #[serde(default)]
    pub webhook_secret: String,
}

fn default_label_filter() -> Vec<String> {
    vec!["INBOX".into()]
}

impl crate::config::traits::ChannelConfig for GmailPushConfig {
    fn name() -> &'static str {
        "Gmail Push"
    }
    fn desc() -> &'static str {
        "Gmail Pub/Sub real-time push notifications"
    }
}

impl Default for GmailPushConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            topic: String::new(),
            label_filter: default_label_filter(),
            oauth_token: String::new(),
            allowed_senders: Vec::new(),
            webhook_url: String::new(),
            webhook_secret: String::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PubSubEnvelope {
    pub message: PubSubMessage,

    #[serde(default)]
    pub subscription: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PubSubMessage {

    pub data: String,

    #[serde(default, rename = "messageId")]
    pub message_id: String,

    #[serde(default, rename = "publishTime")]
    pub publish_time: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GmailNotification {

    #[serde(rename = "emailAddress")]
    pub email_address: String,

    #[serde(rename = "historyId")]
    pub history_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct HistoryResponse {
    pub history: Option<Vec<HistoryRecord>>,
    #[serde(default, rename = "historyId")]
    pub history_id: u64,
    #[serde(default, rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryRecord {
    #[serde(default, rename = "messagesAdded")]
    pub messages_added: Vec<MessageAdded>,
}

#[derive(Debug, Deserialize)]
pub struct MessageAdded {
    pub message: MessageRef,
}

#[derive(Debug, Deserialize)]
pub struct MessageRef {
    pub id: String,
    #[serde(default, rename = "threadId")]
    pub thread_id: String,
}

#[derive(Debug, Deserialize)]
pub struct GmailMessage {
    pub id: String,
    #[serde(default, rename = "threadId")]
    pub thread_id: String,
    #[serde(default)]
    pub snippet: String,
    pub payload: Option<MessagePayload>,
    #[serde(default, rename = "internalDate")]
    pub internal_date: String,
}

#[derive(Debug, Deserialize)]
pub struct MessagePayload {
    #[serde(default)]
    pub headers: Vec<MessageHeader>,
    pub body: Option<MessageBody>,
    #[serde(default)]
    pub parts: Vec<MessagePart>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Deserialize)]
pub struct MessageHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct MessageBody {
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct MessagePart {
    #[serde(default, rename = "mimeType")]
    pub mime_type: String,
    pub body: Option<MessageBody>,
    #[serde(default)]
    pub parts: Vec<MessagePart>,
    #[serde(default)]
    pub filename: String,
}

#[derive(Debug, Deserialize)]
pub struct WatchResponse {
    #[serde(default, rename = "historyId")]
    pub history_id: u64,
    #[serde(default)]
    pub expiration: String,
}

pub struct GmailPushChannel {
    pub config: GmailPushConfig,
    http: Client,
    last_history_id: Arc<Mutex<u64>>,

    pub tx: Arc<Mutex<Option<mpsc::Sender<ChannelMessage>>>>,
}

impl GmailPushChannel {
    pub fn new(config: GmailPushConfig) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            config,
            http,
            last_history_id: Arc::new(Mutex::new(0)),
            tx: Arc::new(Mutex::new(None)),
        }
    }

    pub fn resolve_webhook_secret(&self) -> String {
        if !self.config.webhook_secret.is_empty() {
            return self.config.webhook_secret.clone();
        }
        std::env::var("GMAIL_PUSH_WEBHOOK_SECRET").unwrap_or_default()
    }

    pub fn resolve_oauth_token(&self) -> String {
        if !self.config.oauth_token.is_empty() {
            return self.config.oauth_token.clone();
        }
        std::env::var("GMAIL_PUSH_OAUTH_TOKEN").unwrap_or_default()
    }

    pub async fn register_watch(&self) -> Result<WatchResponse> {
        let token = self.resolve_oauth_token();
        if token.is_empty() {
            return Err(anyhow!("Gmail OAuth token is not configured"));
        }

        let body = serde_json::json!({
            "topicName": self.config.topic,
            "labelIds": self.config.label_filter,
        });

        let resp = self
            .http
            .post("https://gmail.googleapis.com/gmail/v1/users/me/watch")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Gmail watch registration failed ({}): {}",
                status,
                text
            ));
        }

        let watch: WatchResponse = resp.json().await?;
        let mut last_id = self.last_history_id.lock().await;
        if *last_id == 0 {
            *last_id = watch.history_id;
        }
        info!(
            "Gmail watch registered  -  historyId={}, expiration={}",
            watch.history_id, watch.expiration
        );
        Ok(watch)
    }

    pub async fn fetch_history(&self, start_history_id: u64) -> Result<Vec<String>> {
        let mut last_id = self.last_history_id.lock().await;
        self.fetch_history_inner(start_history_id, &mut last_id)
            .await
    }

    async fn fetch_history_inner(
        &self,
        start_history_id: u64,
        last_id: &mut u64,
    ) -> Result<Vec<String>> {
        let token = self.resolve_oauth_token();
        if token.is_empty() {
            return Err(anyhow!("Gmail OAuth token is not configured"));
        }

        let mut message_ids = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/history?startHistoryId={}&historyTypes=messageAdded",
                start_history_id
            );
            if let Some(ref pt) = page_token {
                let _ = write!(url, "&pageToken={pt}");
            }

            let resp = self.http.get(&url).bearer_auth(&token).send().await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Gmail history fetch failed ({}): {}", status, text));
            }

            let history_resp: HistoryResponse = resp.json().await?;

            if let Some(records) = history_resp.history {
                for record in records {
                    for added in record.messages_added {
                        message_ids.push(added.message.id);
                    }
                }
            }

            if history_resp.history_id > 0 && history_resp.history_id > *last_id {
                *last_id = history_resp.history_id;
            }

            match history_resp.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }

        Ok(message_ids)
    }

    pub async fn fetch_message(&self, message_id: &str) -> Result<GmailMessage> {
        let token = self.resolve_oauth_token();
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
            message_id
        );

        let resp = self.http.get(&url).bearer_auth(&token).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Gmail message fetch failed ({}): {}", status, text));
        }

        Ok(resp.json().await?)
    }

    pub fn is_sender_allowed(&self, email: &str) -> bool {
        if self.config.allowed_senders.is_empty() {
            return false;
        }
        if self.config.allowed_senders.iter().any(|a| a == "*") {
            return true;
        }
        let email_lower = email.to_lowercase();
        self.config.allowed_senders.iter().any(|allowed| {
            if allowed.starts_with('@') {
                email_lower.ends_with(&allowed.to_lowercase())
            } else if allowed.contains('@') {
                allowed.eq_ignore_ascii_case(email)
            } else {
                email_lower.ends_with(&format!("@{}", allowed.to_lowercase()))
            }
        })
    }

    pub async fn handle_notification(&self, envelope: &PubSubEnvelope) -> Result<()> {
        let notification = parse_notification(&envelope.message)?;
        debug!(
            "Gmail push notification: email={}, historyId={}",
            notification.email_address, notification.history_id
        );

        let mut last_id = self.last_history_id.lock().await;

        if *last_id == 0 {

            *last_id = notification.history_id;
            info!(
                "Gmail push: first notification, seeding historyId={}",
                notification.history_id
            );
            return Ok(());
        }

        let start_id = *last_id;
        let message_ids = self.fetch_history_inner(start_id, &mut last_id).await?;

        drop(last_id);

        if message_ids.is_empty() {
            debug!("Gmail push: no new messages in history");
            return Ok(());
        }

        info!(
            "Gmail push: {} new message(s) to process",
            message_ids.len()
        );

        let tx = {
            let tx_guard = self.tx.lock().await;
            match tx_guard.clone() {
                Some(tx) => tx,
                None => {
                    warn!("Gmail push: no listener registered, dropping messages");
                    return Ok(());
                }
            }
        };

        for msg_id in message_ids {
            match self.fetch_message(&msg_id).await {
                Ok(gmail_msg) => {
                    let sender = extract_header(&gmail_msg, "From").unwrap_or_default();
                    let sender_email = extract_email_from_header(&sender);

                    if !self.is_sender_allowed(&sender_email) {
                        warn!("Gmail push: blocked message from {}", sender_email);
                        continue;
                    }

                    let subject = extract_header(&gmail_msg, "Subject").unwrap_or_default();
                    let body_text = extract_body_text(&gmail_msg);

                    let content = format!("Subject: {subject}\n\n{body_text}");
                    let timestamp = gmail_msg
                        .internal_date
                        .parse::<u64>()
                        .map(|ms| ms / 1000)
                        .unwrap_or_else(|_| {
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        });

                    let channel_msg = ChannelMessage {
                        id: format!("gmail_{}", gmail_msg.id),
                        reply_target: sender_email.clone(),
                        sender: sender_email,
                        content,
                        channel: "gmail_push".to_string(),
                        timestamp,
                        thread_ts: Some(gmail_msg.thread_id),
                        interruption_scope_id: None,
                        attachments: Vec::new(),
                    };

                    if tx.send(channel_msg).await.is_err() {
                        debug!("Gmail push: listener channel closed");
                        return Ok(());
                    }
                }
                Err(e) => {
                    error!("Gmail push: failed to fetch message {}: {}", msg_id, e);
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Channel for GmailPushChannel {
    fn name(&self) -> &str {
        "gmail_push"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {

        let token = self.resolve_oauth_token();
        if token.is_empty() {
            return Err(anyhow!("Gmail OAuth token is not configured for sending"));
        }

        let subject = message
            .subject
            .as_deref()
            .unwrap_or("SenWeaverCoding Message");

        let safe_recipient = sanitize_header_value(&message.recipient);
        let safe_subject = sanitize_header_value(subject);
        let rfc2822 = format!(
            "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}",
            safe_recipient, safe_subject, message.content
        );
        let encoded = BASE64.encode(rfc2822.as_bytes());

        let url_safe = encoded.replace('+', "-").replace('/', "_").replace('=', "");

        let body = serde_json::json!({
            "raw": url_safe,
        });

        let resp = self
            .http
            .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Gmail send failed ({}): {}", status, text));
        }

        info!("Gmail message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {

        {
            let mut tx_guard = self.tx.lock().await;
            *tx_guard = Some(tx);
        }

        info!("Gmail push channel started  -  registering watch subscription");

        if !self.config.webhook_url.is_empty() {
            if let Err(e) = self.register_watch().await {
                error!("Gmail watch registration failed: {e:#}");

            }
        }

        let renewal_interval = Duration::from_secs(6 * 24 * 60 * 60);
        loop {
            tokio::time::sleep(renewal_interval).await;
            info!("Gmail push: renewing watch subscription");
            if let Err(e) = self.register_watch().await {
                error!("Gmail watch renewal failed: {e:#}");
            }
        }
    }

    async fn health_check(&self) -> bool {
        let token = self.resolve_oauth_token();
        if token.is_empty() {
            return false;
        }

        match self
            .http
            .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
            .bearer_auth(&token)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }
}

pub fn parse_notification(msg: &PubSubMessage) -> Result<GmailNotification> {
    let decoded = BASE64
        .decode(&msg.data)
        .map_err(|e| anyhow!("Invalid base64 in Pub/Sub message: {e}"))?;
    let notification: GmailNotification = serde_json::from_slice(&decoded)
        .map_err(|e| anyhow!("Invalid JSON in Gmail notification: {e}"))?;
    Ok(notification)
}

pub fn extract_header(msg: &GmailMessage, name: &str) -> Option<String> {
    msg.payload.as_ref().and_then(|p| {
        p.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.clone())
    })
}

pub fn extract_email_from_header(from: &str) -> String {
    if let Some(start) = from.find('<') {

        if let Some(end) = from.rfind('>') {
            if end > start + 1 {
                return from[start + 1..end].to_string();
            }
        }
    }
    from.trim().to_string()
}

pub fn sanitize_header_value(value: &str) -> String {
    value.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

pub fn extract_body_text(msg: &GmailMessage) -> String {
    if let Some(ref payload) = msg.payload {

        if payload.mime_type == "text/plain" {
            if let Some(text) = decode_body(payload.body.as_ref()) {
                return text;
            }
        }

        if let Some(text) = find_text_in_parts(&payload.parts, "text/plain") {
            return text;
        }
        if let Some(html) = find_text_in_parts(&payload.parts, "text/html") {
            return strip_html(&html);
        }
    }

    msg.snippet.clone()
}

fn find_text_in_parts(parts: &[MessagePart], mime_type: &str) -> Option<String> {
    for part in parts {
        if part.mime_type == mime_type {
            if let Some(text) = decode_body(part.body.as_ref()) {
                return Some(text);
            }
        }

        if let Some(text) = find_text_in_parts(&part.parts, mime_type) {
            return Some(text);
        }
    }
    None
}

fn decode_body(body: Option<&MessageBody>) -> Option<String> {
    body.and_then(|b| {
        b.data.as_ref().and_then(|data| {

            let standard = data.replace('-', "+").replace('_', "/");

            let padded = match standard.len() % 4 {
                2 => format!("{standard}=="),
                3 => format!("{standard}="),
                _ => standard,
            };
            BASE64
                .decode(&padded)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
    })
}

fn strip_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    let mut normalized = String::with_capacity(result.len());
    for word in result.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(word);
    }
    normalized
}

