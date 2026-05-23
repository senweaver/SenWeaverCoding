// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use reqwest::multipart::{Form, Part};
use serde_json::json;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

pub struct DiscordChannel {
    bot_token: String,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    listen_to_bots: bool,
    mention_only: bool,
    typing_handles: Mutex<HashMap<String, crate::runtime::TaskHandle>>,

    proxy_url: Option<String>,

    transcription: Option<crate::config::TranscriptionConfig>,
    transcription_manager: Option<std::sync::Arc<super::transcription::TranscriptionManager>>,

    stream_mode: crate::config::StreamMode,

    draft_update_interval_ms: u64,

    multi_message_delay_ms: u64,

    last_draft_edit: Mutex<HashMap<String, std::time::Instant>>,

    multi_message_sent_len: Mutex<HashMap<String, usize>>,

    multi_message_thread_ts: Mutex<HashMap<String, Option<String>>>,
}

impl DiscordChannel {
    pub fn new(
        bot_token: String,
        guild_id: Option<String>,
        allowed_users: Vec<String>,
        listen_to_bots: bool,
        mention_only: bool,
    ) -> Self {
        Self {
            bot_token,
            guild_id,
            allowed_users,
            listen_to_bots,
            mention_only,
            typing_handles: Mutex::new(HashMap::new()),
            proxy_url: None,
            transcription: None,
            transcription_manager: None,
            stream_mode: crate::config::StreamMode::Off,
            draft_update_interval_ms: 1000,
            multi_message_delay_ms: 800,
            last_draft_edit: Mutex::new(HashMap::new()),
            multi_message_sent_len: Mutex::new(HashMap::new()),
            multi_message_thread_ts: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    pub fn with_transcription(mut self, config: crate::config::TranscriptionConfig) -> Self {
        if !config.enabled {
            return self;
        }
        match super::transcription::TranscriptionManager::new(&config) {
            Ok(m) => {
                self.transcription_manager = Some(std::sync::Arc::new(m));
                self.transcription = Some(config);
            }
            Err(e) => {
                tracing::warn!(
                    "transcription manager init failed, voice transcription disabled: {e}"
                );
            }
        }
        self
    }

    pub fn with_streaming(
        mut self,
        stream_mode: crate::config::StreamMode,
        draft_update_interval_ms: u64,
        multi_message_delay_ms: u64,
    ) -> Self {
        self.stream_mode = stream_mode;
        self.draft_update_interval_ms = draft_update_interval_ms;
        self.multi_message_delay_ms = multi_message_delay_ms;
        self
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::get_services()
            .proxy_runtime()
            .build_channel_client("channel.discord", self.proxy_url.as_deref())
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    fn bot_user_id_from_token(token: &str) -> Option<String> {

        let part = token.split('.').next()?;
        base64_decode(part)
    }
}

async fn process_attachments(
    attachments: &[serde_json::Value],
    client: &reqwest::Client,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for att in attachments {
        let ct = att
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let name = att
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let Some(url) = att.get("url").and_then(|v| v.as_str()) else {
            tracing::warn!(name, "discord: attachment has no url, skipping");
            continue;
        };
        if ct.starts_with("text/") {
            match client.get(url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(text) = resp.text().await {
                        parts.push(format!("[{name}]\n{text}"));
                    }
                }
                Ok(resp) => {
                    tracing::warn!(name, status = %resp.status(), "discord attachment fetch failed");
                }
                Err(e) => {
                    tracing::warn!(name, error = %e, "discord attachment fetch error");
                }
            }
        } else {
            tracing::debug!(
                name,
                content_type = ct,
                "discord: skipping unsupported attachment type"
            );
        }
    }
    parts.join("\n---\n")
}

const DISCORD_AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "mpeg", "mpga", "mp4", "m4a", "ogg", "oga", "opus", "wav", "webm",
];

fn is_discord_audio_attachment(content_type: &str, filename: &str) -> bool {
    if content_type.starts_with("audio/") {
        return true;
    }
    if let Some(ext) = filename.rsplit('.').next() {
        return DISCORD_AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str());
    }
    false
}

async fn transcribe_discord_audio_attachments(
    attachments: &[serde_json::Value],
    client: &reqwest::Client,
    manager: &super::transcription::TranscriptionManager,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for att in attachments {
        let ct = att
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let name = att
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("file");

        if !is_discord_audio_attachment(ct, name) {
            continue;
        }

        let Some(url) = att.get("url").and_then(|v| v.as_str()) else {
            continue;
        };

        let audio_data = match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => bytes.to_vec(),
                Err(e) => {
                    tracing::warn!(name, error = %e, "discord: failed to read audio attachment bytes");
                    continue;
                }
            },
            Ok(resp) => {
                tracing::warn!(name, status = %resp.status(), "discord: audio attachment download failed");
                continue;
            }
            Err(e) => {
                tracing::warn!(name, error = %e, "discord: audio attachment fetch error");
                continue;
            }
        };

        match manager.transcribe(&audio_data, name).await {
            Ok(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    tracing::info!(
                        "Discord: transcribed audio attachment {} ({} chars)",
                        name,
                        trimmed.len()
                    );
                    parts.push(format!("[Voice] {trimmed}"));
                }
            }
            Err(e) => {
                tracing::warn!(name, error = %e, "discord: voice transcription failed");
            }
        }
    }
    parts.join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiscordAttachmentKind {
    Image,
    Document,
    Video,
    Audio,
    Voice,
}

impl DiscordAttachmentKind {
    fn from_marker(kind: &str) -> Option<Self> {
        match kind.trim().to_ascii_uppercase().as_str() {
            "IMAGE" | "PHOTO" => Some(Self::Image),
            "DOCUMENT" | "FILE" => Some(Self::Document),
            "VIDEO" => Some(Self::Video),
            "AUDIO" => Some(Self::Audio),
            "VOICE" => Some(Self::Voice),
            _ => None,
        }
    }

    fn marker_name(&self) -> &'static str {
        match self {
            Self::Image => "IMAGE",
            Self::Document => "DOCUMENT",
            Self::Video => "VIDEO",
            Self::Audio => "AUDIO",
            Self::Voice => "VOICE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscordAttachment {
    kind: DiscordAttachmentKind,
    target: String,
}

fn parse_attachment_markers(message: &str) -> (String, Vec<DiscordAttachment>) {
    let mut cleaned = String::with_capacity(message.len());
    let mut attachments = Vec::new();
    let mut cursor = 0usize;

    while let Some(rel_start) = message[cursor..].find('[') {
        let start = cursor + rel_start;
        cleaned.push_str(&message[cursor..start]);

        let Some(rel_end) = message[start..].find(']') else {
            cleaned.push_str(&message[start..]);
            cursor = message.len();
            break;
        };
        let end = start + rel_end;
        let marker_text = &message[start + 1..end];

        let parsed = marker_text.split_once(':').and_then(|(kind, target)| {
            let kind = DiscordAttachmentKind::from_marker(kind)?;
            let target = target.trim();
            if target.is_empty() {
                return None;
            }
            Some(DiscordAttachment {
                kind,
                target: target.to_string(),
            })
        });

        if let Some(attachment) = parsed {
            attachments.push(attachment);
        } else {
            cleaned.push_str(&message[start..=end]);
        }

        cursor = end + 1;
    }

    if cursor < message.len() {
        cleaned.push_str(&message[cursor..]);
    }

    (cleaned.trim().to_string(), attachments)
}

fn classify_outgoing_attachments(
    attachments: &[DiscordAttachment],
) -> (Vec<PathBuf>, Vec<String>, Vec<String>) {
    let mut local_files = Vec::new();
    let mut remote_urls = Vec::new();
    let mut unresolved_markers = Vec::new();

    for attachment in attachments {
        let target = attachment.target.trim();
        if target.starts_with("https://") || target.starts_with("http://") {
            remote_urls.push(target.to_string());
            continue;
        }

        let path = Path::new(target);
        if path.exists() && path.is_file() {
            local_files.push(path.to_path_buf());
            continue;
        }

        unresolved_markers.push(format!("[{}:{}]", attachment.kind.marker_name(), target));
    }

    (local_files, remote_urls, unresolved_markers)
}

fn with_inline_attachment_urls(
    content: &str,
    remote_urls: &[String],
    unresolved_markers: &[String],
) -> String {
    let mut lines = Vec::new();
    if !content.trim().is_empty() {
        lines.push(content.trim().to_string());
    }
    if !remote_urls.is_empty() {
        lines.extend(remote_urls.iter().cloned());
    }
    if !unresolved_markers.is_empty() {
        lines.extend(unresolved_markers.iter().cloned());
    }
    lines.join("\n")
}

async fn send_discord_message_json(
    client: &reqwest::Client,
    bot_token: &str,
    recipient: &str,
    content: &str,
) -> anyhow::Result<()> {
    let url = format!("https://discord.com/api/v10/channels/{recipient}/messages");
    let body = json!({ "content": content });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bot {bot_token}"))
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        anyhow::bail!("Discord send message failed ({status}): {err}");
    }

    Ok(())
}

async fn send_discord_message_with_files(
    client: &reqwest::Client,
    bot_token: &str,
    recipient: &str,
    content: &str,
    files: &[PathBuf],
) -> anyhow::Result<()> {
    let url = format!("https://discord.com/api/v10/channels/{recipient}/messages");

    let mut form = Form::new().text("payload_json", json!({ "content": content }).to_string());

    for (idx, path) in files.iter().enumerate() {
        let bytes = tokio::fs::read(path).await.map_err(|error| {
            anyhow::anyhow!(
                "Discord attachment read failed for '{}': {error}",
                path.display()
            )
        })?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment.bin")
            .to_string();
        form = form.part(
            format!("files[{idx}]"),
            Part::bytes(bytes).file_name(filename),
        );
    }

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bot {bot_token}"))
        .multipart(form)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        anyhow::bail!("Discord send message with files failed ({status}): {err}");
    }

    Ok(())
}

async fn send_discord_message_json_with_id(
    client: &reqwest::Client,
    bot_token: &str,
    recipient: &str,
    content: &str,
) -> anyhow::Result<String> {
    let url = format!("https://discord.com/api/v10/channels/{recipient}/messages");
    let body = json!({ "content": content });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bot {bot_token}"))
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        anyhow::bail!("Discord send message failed ({status}): {err}");
    }

    let resp_json: serde_json::Value = resp.json().await?;
    resp_json
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("Discord send response missing 'id' field"))
}

async fn edit_discord_message(
    client: &reqwest::Client,
    bot_token: &str,
    channel_id: &str,
    message_id: &str,
    content: &str,
) -> anyhow::Result<()> {
    let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages/{message_id}");
    let body = json!({ "content": content });

    let resp = client
        .patch(&url)
        .header("Authorization", format!("Bot {bot_token}"))
        .json(&body)
        .send()
        .await?;

    if resp.status().as_u16() == 429 {
        tracing::debug!("Discord edit message rate-limited (429), skipping update");
        return Ok(());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        anyhow::bail!("Discord edit message failed ({status}): {err}");
    }

    Ok(())
}

async fn delete_discord_message(
    client: &reqwest::Client,
    bot_token: &str,
    channel_id: &str,
    message_id: &str,
) -> anyhow::Result<()> {
    let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages/{message_id}");

    let resp = client
        .delete(&url)
        .header("Authorization", format!("Bot {bot_token}"))
        .send()
        .await?;

    if resp.status().as_u16() == 429 {
        tracing::debug!("Discord delete message rate-limited (429), skipping");
        return Ok(());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        anyhow::bail!("Discord delete message failed ({status}): {err}");
    }

    Ok(())
}

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const DISCORD_MAX_MESSAGE_LENGTH: usize = 2000;
const DISCORD_ACK_REACTIONS: &[&str] = &["⚡️", "🦀", "🙌", "💪", "👌", "👀", "👣"];

fn split_message_for_discord(message: &str) -> Vec<String> {
    if message.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {

        let hard_split = remaining
            .char_indices()
            .nth(DISCORD_MAX_MESSAGE_LENGTH)
            .map_or(remaining.len(), |(idx, _)| idx);

        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {

            let search_area = &remaining[..hard_split];

            if let Some(pos) = search_area.rfind('\n') {

                if search_area[..pos].chars().count() >= DISCORD_MAX_MESSAGE_LENGTH / 2 {
                    pos + 1
                } else {

                    search_area.rfind(' ').map_or(hard_split, |space| space + 1)
                }
            } else if let Some(pos) = search_area.rfind(' ') {
                pos + 1
            } else {

                hard_split
            }
        };

        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }

    chunks
}

fn split_message_for_discord_multi(content: &str, max_len: usize) -> Vec<String> {
    if content.is_empty() {
        return vec![];
    }

    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
        }

        if line.is_empty() && !in_fence && !current.is_empty() {
            segments.push(current.trim_end().to_string());
            current.clear();
            continue;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        segments.push(current.trim_end().to_string());
    }

    let mut chunks: Vec<String> = Vec::new();

    for segment in segments {
        if segment.chars().count() > max_len {

            let sub_chunks = split_message_for_discord(&segment);
            chunks.extend(sub_chunks);
        } else {
            chunks.push(segment);
        }
    }

    if chunks.is_empty() {
        vec![content.to_string()]
    } else {
        chunks
    }
}

fn pick_uniform_index(len: usize) -> usize {
    debug_assert!(len > 0);
    let upper = len as u64;
    let reject_threshold = (u64::MAX / upper) * upper;

    loop {
        let value = rand::random::<u64>();
        if value < reject_threshold {
            #[allow(clippy::cast_possible_truncation)]
            return (value % upper) as usize;
        }
    }
}

fn random_discord_ack_reaction() -> &'static str {
    DISCORD_ACK_REACTIONS[pick_uniform_index(DISCORD_ACK_REACTIONS.len())]
}

fn encode_emoji_for_discord(emoji: &str) -> String {
    if emoji.contains(':') {
        return emoji.to_string();
    }

    let mut encoded = String::new();
    for byte in emoji.as_bytes() {
        let _ = write!(encoded, "%{byte:02X}");
    }
    encoded
}

fn discord_reaction_url(channel_id: &str, message_id: &str, emoji: &str) -> String {
    let raw_id = message_id.strip_prefix("discord_").unwrap_or(message_id);
    let encoded_emoji = encode_emoji_for_discord(emoji);
    format!(
        "https://discord.com/api/v10/channels/{channel_id}/messages/{raw_id}/reactions/{encoded_emoji}/@me"
    )
}

fn mention_tags(bot_user_id: &str) -> [String; 2] {
    [format!("<@{bot_user_id}>"), format!("<@!{bot_user_id}>")]
}

fn contains_bot_mention(content: &str, bot_user_id: &str) -> bool {
    let tags = mention_tags(bot_user_id);
    content.contains(&tags[0]) || content.contains(&tags[1])
}

fn normalize_incoming_content(
    content: &str,
    mention_only: bool,
    bot_user_id: &str,
) -> Option<String> {
    if content.is_empty() {
        return None;
    }

    if mention_only && !contains_bot_mention(content, bot_user_id) {
        return None;
    }

    let mut normalized = content.to_string();
    if mention_only {
        for tag in mention_tags(bot_user_id) {
            normalized = normalized.replace(&tag, " ");
        }
    }

    let normalized = normalized.trim().to_string();
    if normalized.is_empty() {
        return None;
    }

    Some(normalized)
}

#[allow(clippy::cast_possible_truncation)]
fn base64_decode(input: &str) -> Option<String> {
    let padded = match input.len() % 4 {
        2 => format!("{input}=="),
        3 => format!("{input}="),
        _ => input.to_string(),
    };

    let mut bytes = Vec::new();
    let chars: Vec<u8> = padded.bytes().collect();

    for chunk in chars.chunks(4) {
        if chunk.len() < 4 {
            break;
        }

        let mut v = [0usize; 4];
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                v[i] = 0;
            } else {
                v[i] = BASE64_ALPHABET.iter().position(|&a| a == b)?;
            }
        }

        bytes.push(((v[0] << 2) | (v[1] >> 4)) as u8);
        if chunk[2] != b'=' {
            bytes.push((((v[1] & 0xF) << 4) | (v[2] >> 2)) as u8);
        }
        if chunk[3] != b'=' {
            bytes.push((((v[2] & 0x3) << 6) | v[3]) as u8);
        }
    }

    String::from_utf8(bytes).ok()
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let raw_content = super::strip_tool_call_tags(&message.content);
        let (cleaned_content, parsed_attachments) = parse_attachment_markers(&raw_content);
        let (mut local_files, remote_urls, unresolved_markers) =
            classify_outgoing_attachments(&parsed_attachments);

        if !unresolved_markers.is_empty() {
            tracing::warn!(
                unresolved = ?unresolved_markers,
                "discord: unresolved attachment markers were sent as plain text"
            );
        }

        if local_files.len() > 10 {
            tracing::warn!(
                count = local_files.len(),
                "discord: truncating local attachment upload list to 10 files"
            );
            local_files.truncate(10);
        }

        let content =
            with_inline_attachment_urls(&cleaned_content, &remote_urls, &unresolved_markers);

        if self.stream_mode == crate::config::StreamMode::MultiMessage {
            let chunks = split_message_for_discord_multi(&content, DISCORD_MAX_MESSAGE_LENGTH);
            let client = self.http_client();

            for (i, chunk) in chunks.iter().enumerate() {
                if i == 0 && !local_files.is_empty() {
                    send_discord_message_with_files(
                        &client,
                        &self.bot_token,
                        &message.recipient,
                        chunk,
                        &local_files,
                    )
                    .await?;
                } else {
                    send_discord_message_json(&client, &self.bot_token, &message.recipient, chunk)
                        .await?;
                }

                if i < chunks.len() - 1 {

                    if message
                        .cancellation_token
                        .as_ref()
                        .is_some_and(|t| t.is_cancelled())
                    {
                        tracing::debug!(
                            "MultiMessage delivery interrupted after chunk {}/{}",
                            i + 1,
                            chunks.len()
                        );
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(
                        self.multi_message_delay_ms,
                    ))
                    .await;
                }
            }

            return Ok(());
        }

        let chunks = split_message_for_discord(&content);
        let client = self.http_client();

        for (i, chunk) in chunks.iter().enumerate() {
            if i == 0 && !local_files.is_empty() {
                send_discord_message_with_files(
                    &client,
                    &self.bot_token,
                    &message.recipient,
                    chunk,
                    &local_files,
                )
                .await?;
            } else {
                send_discord_message_json(&client, &self.bot_token, &message.recipient, chunk)
                    .await?;
            }

            if i < chunks.len() - 1 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let bot_user_id = Self::bot_user_id_from_token(&self.bot_token).unwrap_or_default();

        let gw_resp: serde_json::Value = self
            .http_client()
            .get("https://discord.com/api/v10/gateway/bot")
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await?
            .json()
            .await?;

        let gw_url = gw_resp
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("wss://gateway.discord.gg");

        let ws_url = format!("{gw_url}/?v=10&encoding=json");
        tracing::info!("Discord: connecting to gateway...");

        let (ws_stream, _) = crate::services::get_services()
            .proxy_runtime()
            .ws_connect(&ws_url, "channel.discord", self.proxy_url.as_deref())
            .await?;
        let (mut write, mut read) = ws_stream.split();

        let hello = read.next().await.ok_or(anyhow::anyhow!("No hello"))??;
        let hello_data: serde_json::Value = serde_json::from_str(&hello.to_string())?;
        let heartbeat_interval = hello_data
            .get("d")
            .and_then(|d| d.get("heartbeat_interval"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(41250);

        let identify = json!({
            "op": 2,
            "d": {
                "token": self.bot_token,
                "intents": 37377,
                "properties": {
                    "os": "linux",
                    "browser": "sen",
                    "device": "sen"
                }
            }
        });
        write
            .send(Message::Text(identify.to_string().into()))
            .await?;

        tracing::info!("Discord: connected and identified");

        let mut sequence: i64 = -1;

        let (hb_tx, mut hb_rx) = tokio::sync::mpsc::channel::<()>(1);
        let hb_interval = heartbeat_interval;
        let _heartbeat_task =
            crate::runtime::spawn_supervised("channels.discord.heartbeat", async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_millis(hb_interval));
                loop {
                    interval.tick().await;
                    if hb_tx.send(()).await.is_err() {
                        break;
                    }
                }
            });

        let guild_filter = self.guild_id.clone();

        loop {
            tokio::select! {
                _ = hb_rx.recv() => {
                    let d = if sequence >= 0 { json!(sequence) } else { json!(null) };
                    let hb = json!({"op": 1, "d": d});
                    if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                        break;
                    }
                }
                msg = read.next() => {
                    let msg = match msg {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Ping(payload))) => {
                            if write.send(Message::Pong(payload)).await.is_err() {
                                tracing::warn!("Discord: pong send failed, reconnecting");
                                break;
                            }
                            continue;
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            tracing::warn!("Discord: websocket read error: {e}, reconnecting");
                            break;
                        }
                        _ => continue,
                    };

                    let event: serde_json::Value = match serde_json::from_str(msg.as_ref()) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    if let Some(s) = event.get("s").and_then(serde_json::Value::as_i64) {
                        sequence = s;
                    }

                    let op = event.get("op").and_then(serde_json::Value::as_u64).unwrap_or(0);

                    match op {

                        1 => {
                            let d = if sequence >= 0 { json!(sequence) } else { json!(null) };
                            let hb = json!({"op": 1, "d": d});
                            if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                                break;
                            }
                            continue;
                        }

                        7 => {
                            tracing::warn!("Discord: received Reconnect (op 7), closing for restart");
                            break;
                        }

                        9 => {
                            tracing::warn!("Discord: received Invalid Session (op 9), closing for restart");
                            break;
                        }
                        _ => {}
                    }

                    let event_type = event.get("t").and_then(|t| t.as_str()).unwrap_or("");
                    if event_type != "MESSAGE_CREATE" {
                        continue;
                    }

                    let Some(d) = event.get("d") else {
                        continue;
                    };

                    let author_id = d.get("author").and_then(|a| a.get("id")).and_then(|i| i.as_str()).unwrap_or("");
                    if author_id == bot_user_id {
                        continue;
                    }

                    if !self.listen_to_bots && d.get("author").and_then(|a| a.get("bot")).and_then(serde_json::Value::as_bool).unwrap_or(false) {
                        continue;
                    }

                    if !self.is_user_allowed(author_id) {
                        tracing::warn!("Discord: ignoring message from unauthorized user: {author_id}");
                        continue;
                    }

                    if let Some(ref gid) = guild_filter {
                        let msg_guild = d.get("guild_id").and_then(serde_json::Value::as_str);

                        if let Some(g) = msg_guild {
                            if g != gid {
                                continue;
                            }
                        }
                    }

                    let content = d.get("content").and_then(|c| c.as_str()).unwrap_or("");

                    let is_dm = d.get("guild_id").is_none();
                    let effective_mention_only = self.mention_only && !is_dm;
                    let Some(clean_content) =
                        normalize_incoming_content(content, effective_mention_only, &bot_user_id)
                    else {
                        continue;
                    };

                    let attachment_text = {
                        let atts = d
                            .get("attachments")
                            .and_then(|a| a.as_array())
                            .cloned()
                            .unwrap_or_default();
                        let client = self.http_client();
                        let mut text_parts = process_attachments(&atts, &client).await;

                        if let Some(ref transcription_manager) = self.transcription_manager {
                            let voice_text = transcribe_discord_audio_attachments(
                                &atts,
                                &client,
                                transcription_manager,
                            )
                            .await;
                            if !voice_text.is_empty() {
                                if text_parts.is_empty() {
                                    text_parts = voice_text;
                                } else {
                                    text_parts = format!("{text_parts}
            {voice_text}");
                                }
                            }
                        }

                        text_parts
                    };
                    let final_content = if attachment_text.is_empty() {
                        clean_content
                    } else {
                        format!("{clean_content}\n\n[Attachments]\n{attachment_text}")
                    };

                    let message_id = d.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let channel_id = d
                        .get("channel_id")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();

                    if !message_id.is_empty() && !channel_id.is_empty() {
                        let reaction_channel = DiscordChannel::new(
                            self.bot_token.clone(),
                            self.guild_id.clone(),
                            self.allowed_users.clone(),
                            self.listen_to_bots,
                            self.mention_only,
                        );
                        let reaction_channel_id = channel_id.clone();
                        let reaction_message_id = message_id.to_string();
                        let reaction_emoji = random_discord_ack_reaction().to_string();
                        let _ack_task = crate::runtime::spawn_supervised("channels.discord.reaction_ack", async move {
                            if let Err(err) = reaction_channel
                                .add_reaction(
                                    &reaction_channel_id,
                                    &reaction_message_id,
                                    &reaction_emoji,
                                )
                                .await
                            {
                                tracing::debug!(
                                    "Discord: failed to add ACK reaction for message {reaction_message_id}: {err}"
                                );
                            }
                        });
                    }

                    let channel_msg = ChannelMessage {
                        id: if message_id.is_empty() {
                            Uuid::new_v4().to_string()
                        } else {
                            format!("discord_{message_id}")
                        },
                        sender: author_id.to_string(),
                        reply_target: if channel_id.is_empty() {
                            author_id.to_string()
                        } else {
                            channel_id.clone()
                        },
                        content: final_content,
                        channel: "discord".to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        thread_ts: None,
                        interruption_scope_id: None,
                    attachments: vec![],
                    };

                    if tx.send(channel_msg).await.is_err() {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.http_client()
            .get("https://discord.com/api/v10/users/@me")
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        self.stop_typing(recipient).await?;

        let client = self.http_client();
        let token = self.bot_token.clone();
        let channel_id = recipient.to_string();

        let handle =
            crate::runtime::spawn_supervised("channels.discord.typing_indicator", async move {
                let url = format!("https://discord.com/api/v10/channels/{channel_id}/typing");
                loop {
                    let _ = client
                        .post(&url)
                        .header("Authorization", format!("Bot {token}"))
                        .send()
                        .await;
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                }
            });

        let mut guard = self.typing_handles.lock();
        guard.insert(recipient.to_string(), handle);

        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let mut guard = self.typing_handles.lock();
        if let Some(handle) = guard.remove(recipient) {
            handle.abort();
        }
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        self.stream_mode != crate::config::StreamMode::Off
    }

    fn supports_multi_message_streaming(&self) -> bool {
        self.stream_mode == crate::config::StreamMode::MultiMessage
    }

    fn multi_message_delay_ms(&self) -> u64 {
        self.multi_message_delay_ms
    }

    async fn send_draft(&self, message: &SendMessage) -> anyhow::Result<Option<String>> {
        use crate::config::StreamMode;
        match self.stream_mode {
            StreamMode::Off => Ok(None),
            StreamMode::Partial => {
                let initial_text = if message.content.is_empty() {
                    "...".to_string()
                } else {
                    message.content.clone()
                };

                let client = self.http_client();
                let msg_id = send_discord_message_json_with_id(
                    &client,
                    &self.bot_token,
                    &message.recipient,
                    &initial_text,
                )
                .await?;

                self.last_draft_edit
                    .lock()
                    .insert(message.recipient.clone(), std::time::Instant::now());

                Ok(Some(msg_id))
            }
            StreamMode::MultiMessage => {

                self.multi_message_sent_len.lock().clear();
                self.multi_message_thread_ts
                    .lock()
                    .insert(message.recipient.clone(), message.thread_ts.clone());
                Ok(Some("multi_message_synthetic".to_string()))
            }
        }
    }

    async fn update_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        use crate::config::StreamMode;
        match self.stream_mode {
            StreamMode::Off => Ok(()),
            StreamMode::Partial => {

                {
                    let last_edits = self.last_draft_edit.lock();
                    if let Some(last_time) = last_edits.get(recipient) {
                        let elapsed_ms =
                            u64::try_from(last_time.elapsed().as_millis()).unwrap_or(u64::MAX);
                        if elapsed_ms < self.draft_update_interval_ms {
                            return Ok(());
                        }
                    }
                }

                let display_text = if text.chars().count() > DISCORD_MAX_MESSAGE_LENGTH {
                    let end = text
                        .char_indices()
                        .nth(DISCORD_MAX_MESSAGE_LENGTH)
                        .map(|(i, _)| i)
                        .unwrap_or(text.len());
                    &text[..end]
                } else {
                    text
                };

                let client = self.http_client();
                match edit_discord_message(
                    &client,
                    &self.bot_token,
                    recipient,
                    message_id,
                    display_text,
                )
                .await
                {
                    Ok(()) => {
                        self.last_draft_edit
                            .lock()
                            .insert(recipient.to_string(), std::time::Instant::now());
                    }
                    Err(e) => {
                        tracing::debug!("Discord draft update failed: {e}");
                    }
                }

                Ok(())
            }
            StreamMode::MultiMessage => {

                let (paragraph, thread_ts) = {
                    let thread_ts = self
                        .multi_message_thread_ts
                        .lock()
                        .get(recipient)
                        .cloned()
                        .flatten();
                    let mut sent_map = self.multi_message_sent_len.lock();
                    let sent_so_far = sent_map.get(recipient).copied().unwrap_or(0);

                    if text.len() < sent_so_far {
                        sent_map.insert(recipient.to_string(), 0);
                        return Ok(());
                    }
                    if text.len() == sent_so_far {
                        return Ok(());
                    }

                    let new_text = &text[sent_so_far..];
                    let mut scan_pos = 0;
                    let mut in_fence = false;
                    let bytes = new_text.as_bytes();
                    let mut found_paragraph = None;

                    while scan_pos < bytes.len() {
                        let ch = bytes[scan_pos];

                        if ch == b'`'
                            && scan_pos + 2 < bytes.len()
                            && bytes[scan_pos + 1] == b'`'
                            && bytes[scan_pos + 2] == b'`'
                            && (scan_pos == 0 || bytes[scan_pos - 1] == b'\n')
                        {
                            in_fence = !in_fence;
                        }

                        if !in_fence
                            && ch == b'\n'
                            && scan_pos + 1 < bytes.len()
                            && bytes[scan_pos + 1] == b'\n'
                        {
                            let paragraph = new_text[..scan_pos].trim().to_string();
                            let consumed = scan_pos + 2;
                            *sent_map.entry(recipient.to_string()).or_insert(0) += consumed;
                            if !paragraph.is_empty() {
                                found_paragraph = Some(paragraph);
                            }
                            break;
                        }

                        scan_pos += 1;
                    }

                    (found_paragraph, thread_ts)
                };

                if let Some(paragraph) = paragraph {
                    let msg = SendMessage::new(&paragraph, recipient).in_thread(thread_ts.clone());
                    if let Err(e) = self.send(&msg).await {
                        tracing::debug!("Discord multi-message paragraph send failed: {e}");
                    }
                    if self.multi_message_delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            self.multi_message_delay_ms,
                        ))
                        .await;
                    }

                    return self.update_draft(recipient, message_id, text).await;
                }

                Ok(())
            }
        }
    }

    async fn finalize_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        if self.stream_mode == crate::config::StreamMode::MultiMessage {

            let thread_ts = self
                .multi_message_thread_ts
                .lock()
                .remove(recipient)
                .flatten();
            let sent_so_far = self
                .multi_message_sent_len
                .lock()
                .remove(recipient)
                .unwrap_or(0);
            if text.len() > sent_so_far {
                let remaining = text[sent_so_far..].trim().to_string();
                if !remaining.is_empty() {
                    let msg = SendMessage::new(&remaining, recipient).in_thread(thread_ts);
                    if let Err(e) = self.send(&msg).await {
                        tracing::debug!("Discord multi-message final flush failed: {e}");
                    }
                }
            }
            return Ok(());
        }

        let _ = self.stop_typing(recipient).await;
        self.last_draft_edit.lock().remove(recipient);

        let text = &super::strip_tool_call_tags(text);
        let (cleaned_content, parsed_attachments) = parse_attachment_markers(text);
        let (mut local_files, remote_urls, unresolved_markers) =
            classify_outgoing_attachments(&parsed_attachments);
        let content =
            with_inline_attachment_urls(&cleaned_content, &remote_urls, &unresolved_markers);

        let client = self.http_client();

        if !local_files.is_empty() {
            let _ = delete_discord_message(&client, &self.bot_token, recipient, message_id).await;

            if local_files.len() > 10 {
                local_files.truncate(10);
            }
            let chunks = split_message_for_discord(&content);
            for (i, chunk) in chunks.iter().enumerate() {
                if i == 0 {
                    send_discord_message_with_files(
                        &client,
                        &self.bot_token,
                        recipient,
                        chunk,
                        &local_files,
                    )
                    .await?;
                } else {
                    send_discord_message_json(&client, &self.bot_token, recipient, chunk).await?;
                }
                if i < chunks.len() - 1 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
            return Ok(());
        }

        if content.chars().count() > DISCORD_MAX_MESSAGE_LENGTH {
            let _ = delete_discord_message(&client, &self.bot_token, recipient, message_id).await;

            let chunks = split_message_for_discord(&content);
            for (i, chunk) in chunks.iter().enumerate() {
                send_discord_message_json(&client, &self.bot_token, recipient, chunk).await?;
                if i < chunks.len() - 1 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
            return Ok(());
        }

        if let Err(e) =
            edit_discord_message(&client, &self.bot_token, recipient, message_id, &content).await
        {
            tracing::warn!("Discord finalize_draft edit failed: {e}; falling back to delete+send");
            let _ = delete_discord_message(&client, &self.bot_token, recipient, message_id).await;
            send_discord_message_json(&client, &self.bot_token, recipient, &content).await?;
        }

        Ok(())
    }

    async fn cancel_draft(&self, recipient: &str, message_id: &str) -> anyhow::Result<()> {
        if self.stream_mode == crate::config::StreamMode::MultiMessage {
            self.multi_message_sent_len.lock().remove(recipient);
            self.multi_message_thread_ts.lock().remove(recipient);
            return Ok(());
        }

        let _ = self.stop_typing(recipient).await;
        self.last_draft_edit.lock().remove(recipient);

        let client = self.http_client();
        if let Err(e) =
            delete_discord_message(&client, &self.bot_token, recipient, message_id).await
        {
            tracing::debug!("Discord cancel_draft delete failed: {e}");
        }

        Ok(())
    }

    async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let url = discord_reaction_url(channel_id, message_id, emoji);

        let resp = self
            .http_client()
            .put(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .header("Content-Length", "0")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            anyhow::bail!("Discord add reaction failed ({status}): {err}");
        }

        Ok(())
    }

    async fn remove_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let url = discord_reaction_url(channel_id, message_id, emoji);

        let resp = self
            .http_client()
            .delete(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            anyhow::bail!("Discord remove reaction failed ({status}): {err}");
        }

        Ok(())
    }
}
