// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Channel, ChannelMessage, SendMessage};
use crate::config::{Config, StreamMode};
use crate::security::pairing::PairingGuard;
use anyhow::Context;
use async_trait::async_trait;
use directories::UserDirs;
use parking_lot::Mutex;
use reqwest::multipart::{Form, Part};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::fs;

const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4096;

const TELEGRAM_CONTINUATION_OVERHEAD: usize = 30;
const TELEGRAM_ACK_REACTIONS: &[&str] = &["⚡️", "👌", "👀", "🔥", "👍"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct IncomingAttachment {
    file_id: String,
    file_name: Option<String>,
    file_size: Option<u64>,
    caption: Option<String>,
    kind: IncomingAttachmentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncomingAttachmentKind {
    Document,
    Photo,
}
const TELEGRAM_BIND_COMMAND: &str = "/bind";

fn split_message_for_telegram(message: &str) -> Vec<String> {
    if message.chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;
    let chunk_limit = TELEGRAM_MAX_MESSAGE_LENGTH - TELEGRAM_CONTINUATION_OVERHEAD;

    while !remaining.is_empty() {

        if remaining.chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH {
            chunks.push(remaining.to_string());
            break;
        }

        let hard_split = remaining
            .char_indices()
            .nth(chunk_limit)
            .map_or(remaining.len(), |(idx, _)| idx);

        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {

            let search_area = &remaining[..hard_split];

            if let Some(pos) = search_area.rfind('\n') {

                if search_area[..pos].chars().count() >= chunk_limit / 2 {
                    pos + 1
                } else {

                    search_area.rfind(' ').unwrap_or(hard_split) + 1
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

fn random_telegram_ack_reaction() -> &'static str {
    TELEGRAM_ACK_REACTIONS[pick_uniform_index(TELEGRAM_ACK_REACTIONS.len())]
}

fn build_telegram_ack_reaction_request(
    chat_id: &str,
    message_id: i64,
    emoji: &str,
) -> serde_json::Value {
    serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction": [{
            "type": "emoji",
            "emoji": emoji
        }]
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegramAttachmentKind {
    Image,
    Document,
    Video,
    Audio,
    Voice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelegramAttachment {
    kind: TelegramAttachmentKind,
    target: String,
}

impl TelegramAttachmentKind {
    fn from_marker(marker: &str) -> Option<Self> {
        match marker.trim().to_ascii_uppercase().as_str() {
            "IMAGE" | "PHOTO" => Some(Self::Image),
            "DOCUMENT" | "FILE" => Some(Self::Document),
            "VIDEO" => Some(Self::Video),
            "AUDIO" => Some(Self::Audio),
            "VOICE" => Some(Self::Voice),
            _ => None,
        }
    }
}

fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
            )
        })
        .unwrap_or(false)
}

fn format_attachment_content(
    kind: IncomingAttachmentKind,
    local_filename: &str,
    local_path: &Path,
) -> String {
    match kind {
        IncomingAttachmentKind::Photo | IncomingAttachmentKind::Document
            if is_image_extension(local_path) =>
        {
            format!("[IMAGE:{}]", local_path.display())
        }
        _ => {
            format!("[Document: {}] {}", local_filename, local_path.display())
        }
    }
}

fn is_http_url(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

fn infer_attachment_kind_from_target(target: &str) -> Option<TelegramAttachmentKind> {
    let normalized = target
        .split('?')
        .next()
        .unwrap_or(target)
        .split('#')
        .next()
        .unwrap_or(target);

    let extension = Path::new(normalized)
        .extension()
        .and_then(|ext| ext.to_str())?
        .to_ascii_lowercase();

    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => Some(TelegramAttachmentKind::Image),
        "mp4" | "mov" | "mkv" | "avi" | "webm" => Some(TelegramAttachmentKind::Video),
        "mp3" | "m4a" | "wav" | "flac" => Some(TelegramAttachmentKind::Audio),
        "ogg" | "oga" | "opus" => Some(TelegramAttachmentKind::Voice),
        "pdf" | "txt" | "md" | "csv" | "json" | "zip" | "tar" | "gz" | "doc" | "docx" | "xls"
        | "xlsx" | "ppt" | "pptx" => Some(TelegramAttachmentKind::Document),
        _ => None,
    }
}

fn parse_path_only_attachment(message: &str) -> Option<TelegramAttachment> {
    let trimmed = message.trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return None;
    }

    let candidate = trimmed.trim_matches(|c| matches!(c, '`' | '"' | '\''));
    if candidate.chars().any(char::is_whitespace) {
        return None;
    }

    let candidate = candidate.strip_prefix("file://").unwrap_or(candidate);
    let kind = infer_attachment_kind_from_target(candidate)?;

    if !is_http_url(candidate) && !Path::new(candidate).exists() {
        return None;
    }

    Some(TelegramAttachment {
        kind,
        target: candidate.to_string(),
    })
}

fn strip_tool_call_tags(message: &str) -> String {
    super::strip_tool_call_tags(message)
}

fn find_matching_close(s: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_attachment_markers(message: &str) -> (String, Vec<TelegramAttachment>) {
    let mut cleaned = String::with_capacity(message.len());
    let mut attachments = Vec::new();
    let mut cursor = 0;

    while cursor < message.len() {
        let Some(open_rel) = message[cursor..].find('[') else {
            cleaned.push_str(&message[cursor..]);
            break;
        };

        let open = cursor + open_rel;
        cleaned.push_str(&message[cursor..open]);

        let Some(close_rel) = find_matching_close(&message[open + 1..]) else {
            cleaned.push_str(&message[open..]);
            break;
        };

        let close = open + 1 + close_rel;
        let marker = &message[open + 1..close];

        let parsed = marker.split_once(':').and_then(|(kind, target)| {
            let kind = TelegramAttachmentKind::from_marker(kind)?;
            let target = target.trim();
            if target.is_empty() {
                return None;
            }
            Some(TelegramAttachment {
                kind,
                target: target.to_string(),
            })
        });

        if let Some(attachment) = parsed {
            attachments.push(attachment);
        } else {
            cleaned.push_str(&message[open..=close]);
        }

        cursor = close + 1;
    }

    (cleaned.trim().to_string(), attachments)
}

const TELEGRAM_MAX_FILE_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;

pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Arc<RwLock<Vec<String>>>,
    pairing: Option<PairingGuard>,
    client: reqwest::Client,
    typing_handle: Mutex<Option<crate::runtime::TaskHandle>>,
    stream_mode: StreamMode,
    draft_update_interval_ms: u64,
    last_draft_edit: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    mention_only: bool,
    bot_username: Mutex<Option<String>>,

    api_base: String,
    transcription: Option<crate::config::TranscriptionConfig>,
    transcription_manager: Option<std::sync::Arc<super::pipeline::transcription::TranscriptionManager>>,
    voice_transcriptions: Mutex<std::collections::HashMap<String, String>>,
    workspace_dir: Option<std::path::PathBuf>,
    ack_reactions: bool,
    tts_config: Option<crate::config::TtsConfig>,
    voice_chats: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pending_voice:
        Arc<std::sync::Mutex<std::collections::HashMap<String, (String, std::time::Instant)>>>,

    proxy_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditMessageResult {
    Success,
    NotModified,
    Failed(reqwest::StatusCode),
}

impl TelegramChannel {
    pub fn new(bot_token: String, allowed_users: Vec<String>, mention_only: bool) -> Self {
        let normalized_allowed = Self::normalize_allowed_users(allowed_users);
        let pairing = if normalized_allowed.is_empty() {
            let guard = PairingGuard::new(true, &[]);
            if let Some(code) = guard.pairing_code() {
                println!("  🔐 Telegram pairing required. One-time bind code: {code}");
                println!("     Send `{TELEGRAM_BIND_COMMAND} <code>` from your Telegram account.");
            }
            Some(guard)
        } else {
            None
        };

        Self {
            bot_token,
            allowed_users: Arc::new(RwLock::new(normalized_allowed)),
            pairing,
            client: reqwest::Client::new(),
            stream_mode: StreamMode::Off,
            draft_update_interval_ms: 1000,
            last_draft_edit: Mutex::new(std::collections::HashMap::new()),
            typing_handle: Mutex::new(None),
            mention_only,
            bot_username: Mutex::new(None),
            api_base: "https://api.telegram.org".to_string(),
            transcription: None,
            transcription_manager: None,
            voice_transcriptions: Mutex::new(std::collections::HashMap::new()),
            workspace_dir: None,
            ack_reactions: true,
            tts_config: None,
            voice_chats: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            pending_voice: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            proxy_url: None,
        }
    }

    pub fn with_ack_reactions(mut self, enabled: bool) -> Self {
        self.ack_reactions = enabled;
        self
    }

    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    pub fn with_workspace_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.workspace_dir = Some(dir);
        self
    }

    pub fn with_streaming(
        mut self,
        stream_mode: StreamMode,
        draft_update_interval_ms: u64,
    ) -> Self {
        self.stream_mode = stream_mode;
        self.draft_update_interval_ms = draft_update_interval_ms;
        self
    }

    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.api_base = api_base;
        self
    }

    pub fn with_transcription(mut self, config: crate::config::TranscriptionConfig) -> Self {
        if !config.enabled {
            return self;
        }
        match super::pipeline::transcription::TranscriptionManager::new(&config) {
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

    pub fn with_tts(mut self, config: crate::config::TtsConfig) -> Self {
        if config.enabled {
            self.tts_config = Some(config);
        }
        self
    }

    fn parse_reply_target(reply_target: &str) -> (String, Option<String>) {
        if let Some((chat_id, thread_id)) = reply_target.split_once(':') {
            (chat_id.to_string(), Some(thread_id.to_string()))
        } else {
            (reply_target.to_string(), None)
        }
    }

    fn extract_update_message_target(update: &serde_json::Value) -> Option<(String, i64)> {
        let message = update.get("message")?;
        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)?
            .to_string();
        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)?;
        Some((chat_id, message_id))
    }

    fn try_add_ack_reaction_nonblocking(&self, chat_id: String, message_id: i64) {
        let client = self.http_client();
        let url = self.api_url("setMessageReaction");
        let emoji = random_telegram_ack_reaction().to_string();
        let body = build_telegram_ack_reaction_request(&chat_id, message_id, &emoji);

        let _ = crate::runtime::spawn_supervised("channels.telegram.ack_reaction", async move {
            let response = match client.post(&url).json(&body).send().await {
                Ok(resp) => resp,
                Err(err) => {
                    tracing::warn!(
                        "Telegram: failed to add ACK reaction to chat_id={chat_id}, message_id={message_id}: {err}"
                    );
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let err_body = response.text().await.unwrap_or_default();
                tracing::warn!(
                    "Telegram: add ACK reaction failed for chat_id={chat_id}, message_id={message_id}: status={status}, body={err_body}"
                );
            }
        });
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_channel_client("channel.telegram", self.proxy_url.as_deref())
    }

    fn normalize_identity(value: &str) -> String {
        value.trim().trim_start_matches('@').to_string()
    }

    fn normalize_allowed_users(allowed_users: Vec<String>) -> Vec<String> {
        allowed_users
            .into_iter()
            .map(|entry| Self::normalize_identity(&entry))
            .filter(|entry| !entry.is_empty())
            .collect()
    }

    async fn load_config_without_env() -> anyhow::Result<Config> {
        let home = UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .context("Could not find home directory")?;
        let sen_dir = home.join(".senweavercoding");
        let config_path = sen_dir.join("config.toml");

        let contents = fs::read_to_string(&config_path)
            .await
            .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;
        let mut config: Config = toml::from_str(&contents).context(
            "Failed to parse config.toml  -  check [channels.telegram] section for syntax errors",
        )?;
        config.config_path = config_path;
        config.workspace_dir = sen_dir.join("workspace");
        Ok(config)
    }

    async fn persist_allowed_identity(&self, identity: &str) -> anyhow::Result<()> {
        let mut config = Self::load_config_without_env().await?;
        let Some(telegram) = config.channels_config.telegram.as_mut() else {
            anyhow::bail!(
                "Missing [channels.telegram] section in config.toml. \
                Add bot_token and allowed_users under [channels.telegram], \
                or run `sen onboard --channels-only` to configure interactively"
            );
        };

        let normalized = Self::normalize_identity(identity);
        if normalized.is_empty() {
            anyhow::bail!("Cannot persist empty Telegram identity");
        }

        if !telegram.allowed_users.iter().any(|u| u == &normalized) {
            telegram.allowed_users.push(normalized);
            config
                .save()
                .await
                .context("Failed to persist Telegram allowlist to config.toml")?;
        }

        Ok(())
    }

    fn add_allowed_identity_runtime(&self, identity: &str) {
        let normalized = Self::normalize_identity(identity);
        if normalized.is_empty() {
            return;
        }
        if let Ok(mut users) = self.allowed_users.write() {
            if !users.iter().any(|u| u == &normalized) {
                users.push(normalized);
            }
        }
    }

    fn extract_bind_code(text: &str) -> Option<&str> {
        let mut parts = text.split_whitespace();
        let command = parts.next()?;
        let base_command = command.split('@').next().unwrap_or(command);
        if base_command != TELEGRAM_BIND_COMMAND {
            return None;
        }
        parts.next().map(str::trim).filter(|code| !code.is_empty())
    }

    fn pairing_code_active(&self) -> bool {
        self.pairing
            .as_ref()
            .and_then(PairingGuard::pairing_code)
            .is_some()
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.api_base, self.bot_token)
    }

    async fn synthesize_and_send_voice(
        api_base: &str,
        bot_token: &str,
        chat_id: &str,
        thread_id: Option<&str>,
        text: &str,
        tts_config: &crate::config::TtsConfig,
    ) -> anyhow::Result<()> {
        let tts_manager = super::pipeline::tts::TtsManager::new(tts_config)?;
        let audio_bytes = tts_manager.synthesize(text).await?;
        let audio_len = audio_bytes.len();
        tracing::info!("Telegram TTS: synthesized {audio_len} bytes of audio");

        if audio_bytes.is_empty() {
            anyhow::bail!("TTS returned empty audio");
        }

        let url = format!("{api_base}/bot{bot_token}/sendVoice");
        let client = crate::services::require_services()
            .proxy_runtime()
            .build_client("channel.telegram");

        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part(
                "voice",
                reqwest::multipart::Part::bytes(audio_bytes)
                    .file_name("voice.ogg")
                    .mime_str("audio/ogg")?,
            );

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        let resp = client.post(&url).multipart(form).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("sendVoice failed: status={status}, body={body}");
        }

        tracing::info!("Telegram TTS: sent voice note ({audio_len} bytes)");
        Ok(())
    }

    async fn classify_edit_message_response(resp: reqwest::Response) -> EditMessageResult {
        if resp.status().is_success() {
            return EditMessageResult::Success;
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if body.contains("message is not modified") {
            return EditMessageResult::NotModified;
        }

        EditMessageResult::Failed(status)
    }

    async fn fetch_bot_username(&self) -> anyhow::Result<String> {
        let resp = self.http_client().get(self.api_url("getMe")).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch bot info: {}", resp.status());
        }

        let data: serde_json::Value = resp.json().await?;
        let username = data
            .get("result")
            .and_then(|r| r.get("username"))
            .and_then(|u| u.as_str())
            .context("Bot username not found in response")?;

        Ok(username.to_string())
    }

    async fn get_bot_username(&self) -> Option<String> {
        {
            let cache = self.bot_username.lock();
            if let Some(ref username) = *cache {
                return Some(username.clone());
            }
        }

        match self.fetch_bot_username().await {
            Ok(username) => {
                let mut cache = self.bot_username.lock();
                *cache = Some(username.clone());
                Some(username)
            }
            Err(e) => {
                tracing::warn!("Failed to fetch bot username: {e}");
                None
            }
        }
    }

    fn is_telegram_username_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_'
    }

    fn find_bot_mention_spans(text: &str, bot_username: &str) -> Vec<(usize, usize)> {
        let bot_username = bot_username.trim_start_matches('@');
        if bot_username.is_empty() {
            return Vec::new();
        }

        let mut spans = Vec::new();

        for (at_idx, ch) in text.char_indices() {
            if ch != '@' {
                continue;
            }

            if at_idx > 0 {
                let prev = text[..at_idx].chars().next_back().unwrap_or(' ');
                if Self::is_telegram_username_char(prev) {
                    continue;
                }
            }

            let username_start = at_idx + 1;
            let mut username_end = username_start;

            for (rel_idx, candidate_ch) in text[username_start..].char_indices() {
                if Self::is_telegram_username_char(candidate_ch) {
                    username_end = username_start + rel_idx + candidate_ch.len_utf8();
                } else {
                    break;
                }
            }

            if username_end == username_start {
                continue;
            }

            let mention_username = &text[username_start..username_end];
            if mention_username.eq_ignore_ascii_case(bot_username) {
                spans.push((at_idx, username_end));
            }
        }

        spans
    }

    fn contains_bot_mention(text: &str, bot_username: &str) -> bool {
        !Self::find_bot_mention_spans(text, bot_username).is_empty()
    }

    fn normalize_incoming_content(text: &str, bot_username: &str) -> Option<String> {
        let spans = Self::find_bot_mention_spans(text, bot_username);
        if spans.is_empty() {
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            return (!normalized.is_empty()).then_some(normalized);
        }

        let mut normalized = String::with_capacity(text.len());
        let mut cursor = 0;
        for (start, end) in spans {
            normalized.push_str(&text[cursor..start]);
            cursor = end;
        }
        normalized.push_str(&text[cursor..]);

        let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
        (!normalized.is_empty()).then_some(normalized)
    }

    fn is_group_message(message: &serde_json::Value) -> bool {
        message
            .get("chat")
            .and_then(|c| c.get("type"))
            .and_then(|t| t.as_str())
            .map(|t| t == "group" || t == "supergroup")
            .unwrap_or(false)
    }

    fn is_user_allowed(&self, username: &str) -> bool {
        let identity = Self::normalize_identity(username);
        self.allowed_users
            .read()
            .map(|users| users.iter().any(|u| u == "*" || u == &identity))
            .unwrap_or(false)
    }

    fn is_any_user_allowed<'a, I>(&self, identities: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        identities.into_iter().any(|id| self.is_user_allowed(id))
    }

    async fn handle_unauthorized_message(&self, update: &serde_json::Value) {
        let Some(message) = update.get("message") else {
            return;
        };

        let Some(text) = message.get("text").and_then(serde_json::Value::as_str) else {
            return;
        };

        let username_opt = message
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str);
        let username = username_opt.unwrap_or("unknown");
        let normalized_username = Self::normalize_identity(username);

        let sender_id = message
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64);
        let sender_id_str = sender_id.map(|id| id.to_string());
        let normalized_sender_id = sender_id_str.as_deref().map(Self::normalize_identity);

        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());

        let Some(chat_id) = chat_id else {
            tracing::warn!("Telegram: missing chat_id in message, skipping");
            return;
        };

        let mut identities = vec![normalized_username.as_str()];
        if let Some(ref id) = normalized_sender_id {
            identities.push(id.as_str());
        }

        if self.is_any_user_allowed(identities.iter().copied()) {
            return;
        }

        if let Some(code) = Self::extract_bind_code(text) {
            if let Some(pairing) = self.pairing.as_ref() {
                match pairing.try_pair(code, &chat_id).await {
                    Ok(Some(_token)) => {
                        let bind_identity = normalized_sender_id.clone().or_else(|| {
                            if normalized_username.is_empty() || normalized_username == "unknown" {
                                None
                            } else {
                                Some(normalized_username.clone())
                            }
                        });

                        if let Some(identity) = bind_identity {
                            self.add_allowed_identity_runtime(&identity);
                            match Box::pin(self.persist_allowed_identity(&identity)).await {
                                Ok(()) => {
                                    let _ = self
                                        .send(&SendMessage::new(
                                            "✅ Telegram account bound successfully. You can talk to SenWeaverCoding now.",
                                            &chat_id,
                                        ))
                                        .await;
                                    tracing::info!(
                                        "Telegram: paired and allowlisted identity={identity}"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Telegram: failed to persist allowlist after bind: {e}"
                                    );
                                    let _ = self
                                        .send(&SendMessage::new(
                                            "⚠️ Bound for this runtime, but failed to persist config. Access may be lost after restart; check config file permissions.",
                                            &chat_id,
                                        ))
                                        .await;
                                }
                            }
                        } else {
                            let _ = self
                                .send(&SendMessage::new(
                                    "❌ Could not identify your Telegram account. Ensure your account has a username or stable user ID, then retry.",
                                    &chat_id,
                                ))
                                .await;
                        }
                    }
                    Ok(None) => {
                        let _ = self
                            .send(&SendMessage::new(
                                "❌ Invalid binding code. Ask operator for the latest code and retry.",
                                &chat_id,
                            ))
                            .await;
                    }
                    Err(lockout_secs) => {
                        let _ = self
                            .send(&SendMessage::new(
                                format!("⏳ Too many invalid attempts. Retry in {lockout_secs}s."),
                                &chat_id,
                            ))
                            .await;
                    }
                }
            } else {
                let _ = self
                    .send(&SendMessage::new(
                        "ℹ️ Telegram pairing is not active. Ask operator to add your user ID to channels.telegram.allowed_users in config.toml.",
                        &chat_id,
                    ))
                    .await;
            }
            return;
        }

        tracing::warn!(
            "Telegram: ignoring message from unauthorized user: username={username}, sender_id={}. \
Allowlist Telegram username (without '@') or numeric user ID.",
            sender_id_str.as_deref().unwrap_or("unknown")
        );

        let suggested_identity = normalized_sender_id
            .clone()
            .or_else(|| {
                if normalized_username.is_empty() || normalized_username == "unknown" {
                    None
                } else {
                    Some(normalized_username.clone())
                }
            })
            .unwrap_or_else(|| "YOUR_TELEGRAM_ID".to_string());

        let _ = self
            .send(&SendMessage::new(
                format!(
                    "🔐 This bot requires operator approval.\n\nCopy this command to operator terminal:\n`sen channel bind-telegram {suggested_identity}`\n\nAfter operator runs it, send your message again."
                ),
                &chat_id,
            ))
            .await;

        if self.pairing_code_active() {
            let _ = self
                .send(&SendMessage::new(
                    "ℹ️ If operator provides a one-time pairing code, you can also run `/bind <code>`.",
                    &chat_id,
                ))
                .await;
        }
    }

    async fn get_file_path(&self, file_id: &str) -> anyhow::Result<String> {
        let url = self.api_url("getFile");
        let resp = self
            .http_client()
            .get(&url)
            .query(&[("file_id", file_id)])
            .send()
            .await
            .context("Failed to call Telegram getFile")?;

        let data: serde_json::Value = resp.json().await?;
        data.get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .map(String::from)
            .context("Telegram getFile: missing file_path in response")
    }

    async fn download_file(&self, file_path: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!(
            "https://api.telegram.org/file/bot{}/{file_path}",
            self.bot_token
        );
        let resp = self
            .http_client()
            .get(&url)
            .send()
            .await
            .context("Failed to download Telegram file")?;

        if !resp.status().is_success() {
            anyhow::bail!("Telegram file download failed: {}", resp.status());
        }

        Ok(resp.bytes().await?.to_vec())
    }

    fn parse_voice_metadata(message: &serde_json::Value) -> Option<(String, u64)> {
        let voice = message.get("voice").or_else(|| message.get("audio"))?;
        let file_id = voice.get("file_id")?.as_str()?.to_string();
        let duration = voice
            .get("duration")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        Some((file_id, duration))
    }

    fn parse_attachment_metadata(message: &serde_json::Value) -> Option<IncomingAttachment> {

        if let Some(doc) = message.get("document") {
            let file_id = doc.get("file_id")?.as_str()?.to_string();
            let file_name = doc
                .get("file_name")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            let file_size = doc.get("file_size").and_then(serde_json::Value::as_u64);
            let caption = message
                .get("caption")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            return Some(IncomingAttachment {
                file_id,
                file_name,
                file_size,
                caption,
                kind: IncomingAttachmentKind::Document,
            });
        }

        if let Some(photos) = message.get("photo").and_then(serde_json::Value::as_array) {
            let best = photos.last()?;
            let file_id = best.get("file_id")?.as_str()?.to_string();
            let file_size = best.get("file_size").and_then(serde_json::Value::as_u64);
            let caption = message
                .get("caption")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            return Some(IncomingAttachment {
                file_id,
                file_name: None,
                file_size,
                caption,
                kind: IncomingAttachmentKind::Photo,
            });
        }

        None
    }

    async fn try_parse_attachment_message(
        &self,
        update: &serde_json::Value,
    ) -> Option<ChannelMessage> {
        let message = update.get("message")?;
        let attachment = Self::parse_attachment_metadata(message)?;

        if let Some(size) = attachment.file_size {
            if size > TELEGRAM_MAX_FILE_DOWNLOAD_BYTES {
                tracing::info!(
                    "Skipping attachment: file size {size} bytes exceeds {} MB limit",
                    TELEGRAM_MAX_FILE_DOWNLOAD_BYTES / (1024 * 1024)
                );
                return None;
            }
        }

        let (username, sender_id, sender_identity) = Self::extract_sender_info(message);

        let mut identities = vec![username.as_str()];
        if let Some(id) = sender_id.as_deref() {
            identities.push(id);
        }

        if !self.is_any_user_allowed(identities.iter().copied()) {
            return None;
        }

        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string())?;

        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        let thread_id = message
            .get("message_thread_id")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());

        let reply_target = if let Some(ref tid) = thread_id {
            format!("{}:{}", chat_id, tid)
        } else {
            chat_id.clone()
        };

        let workspace = self.workspace_dir.as_ref().or_else(|| {
            tracing::warn!("Cannot save attachment: workspace_dir not configured");
            None
        })?;

        let save_dir = workspace.join("telegram_files");
        if let Err(e) = tokio::fs::create_dir_all(&save_dir).await {
            tracing::warn!("Failed to create telegram_files directory: {e}");
            return None;
        }

        let tg_file_path = match self.get_file_path(&attachment.file_id).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to get attachment file path: {e}");
                return None;
            }
        };

        let file_data = match self.download_file(&tg_file_path).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to download attachment: {e}");
                return None;
            }
        };

        let local_filename = match &attachment.file_name {
            Some(name) => name.clone(),
            None => {

                let ext = tg_file_path.rsplit('.').next().unwrap_or("jpg");
                format!("photo_{chat_id}_{message_id}.{ext}")
            }
        };

        let local_path = save_dir.join(&local_filename);
        if let Err(e) = tokio::fs::write(&local_path, &file_data).await {
            tracing::warn!("Failed to save attachment to {}: {e}", local_path.display());
            return None;
        }

        let mut content = format_attachment_content(attachment.kind, &local_filename, &local_path);
        if let Some(caption) = &attachment.caption {
            if !caption.is_empty() {
                use std::fmt::Write;
                let _ = write!(content, "\n\n{caption}");
            }
        }

        if let Some(quote) = self.extract_reply_context(message) {
            content = format!("{quote}\n\n{content}");
        }

        if let Some(attr) = Self::format_forward_attribution(message) {
            content = format!("{attr}{content}");
        }

        Some(ChannelMessage {
            id: format!("telegram_{chat_id}_{message_id}"),
            sender: sender_identity,
            reply_target,
            content,
            channel: "telegram".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: thread_id,
            interruption_scope_id: None,
            attachments: vec![],
        })
    }

    async fn try_parse_voice_message(&self, update: &serde_json::Value) -> Option<ChannelMessage> {
        let config = self.transcription.as_ref()?;
        let manager = self.transcription_manager.as_deref()?;
        let message = update.get("message")?;

        let (file_id, duration) = Self::parse_voice_metadata(message)?;

        if duration > config.max_duration_secs {
            tracing::info!(
                "Skipping voice message: duration {duration}s exceeds limit {}s",
                config.max_duration_secs
            );
            return None;
        }

        let (username, sender_id, sender_identity) = Self::extract_sender_info(message);

        let mut identities = vec![username.as_str()];
        if let Some(id) = sender_id.as_deref() {
            identities.push(id);
        }

        if !self.is_any_user_allowed(identities.iter().copied()) {
            return None;
        }

        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string())?;

        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        let thread_id = message
            .get("message_thread_id")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());

        let reply_target = if let Some(ref tid) = thread_id {
            format!("{}:{}", chat_id, tid)
        } else {
            chat_id.clone()
        };

        let file_path = match self.get_file_path(&file_id).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to get voice file path: {e}");
                return None;
            }
        };

        let file_name = file_path
            .rsplit('/')
            .next()
            .unwrap_or("voice.ogg")
            .to_string();

        let audio_data = match self.download_file(&file_path).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to download voice file: {e}");
                return None;
            }
        };

        let text = match manager.transcribe(&audio_data, &file_name).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Voice transcription failed: {e}");
                return None;
            }
        };

        if text.trim().is_empty() {
            tracing::info!("Voice transcription returned empty text, skipping");
            return None;
        }

        if let Ok(mut vc) = self.voice_chats.lock() {
            vc.insert(reply_target.clone());
        }

        {
            let mut cache = self.voice_transcriptions.lock();
            if cache.len() >= 100 {
                cache.clear();
            }
            cache.insert(format!("{chat_id}:{message_id}"), text.clone());
        }

        let content = if let Some(quote) = self.extract_reply_context(message) {
            format!("{quote}\n\n[Voice] {text}")
        } else {
            format!("[Voice] {text}")
        };

        let content = if let Some(attr) = Self::format_forward_attribution(message) {
            format!("{attr}{content}")
        } else {
            content
        };

        Some(ChannelMessage {
            id: format!("telegram_{chat_id}_{message_id}"),
            sender: sender_identity,
            reply_target,
            content,
            channel: "telegram".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: thread_id,
            interruption_scope_id: None,
            attachments: vec![],
        })
    }

    fn extract_sender_info(message: &serde_json::Value) -> (String, Option<String>, String) {
        let username = message
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let sender_id = message
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());
        let sender_identity = if username == "unknown" {
            sender_id.clone().unwrap_or_else(|| "unknown".to_string())
        } else {
            username.clone()
        };
        (username, sender_id, sender_identity)
    }

    fn format_forward_attribution(message: &serde_json::Value) -> Option<String> {
        if let Some(from_chat) = message.get("forward_from_chat") {

            let title = from_chat
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown channel");
            Some(format!("[Forwarded from channel: {title}] "))
        } else if let Some(from_user) = message.get("forward_from") {

            let label = from_user
                .get("username")
                .and_then(serde_json::Value::as_str)
                .map(|u| format!("@{u}"))
                .or_else(|| {
                    from_user
                        .get("first_name")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from)
                })
                .unwrap_or_else(|| "unknown".to_string());
            Some(format!("[Forwarded from {label}] "))
        } else {

            message
                .get("forward_sender_name")
                .and_then(serde_json::Value::as_str)
                .map(|name| format!("[Forwarded from {name}] "))
        }
    }

    fn extract_reply_context(&self, message: &serde_json::Value) -> Option<String> {
        let reply = message.get("reply_to_message")?;

        let reply_sender = reply
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                reply
                    .get("from")
                    .and_then(|from| from.get("first_name"))
                    .and_then(serde_json::Value::as_str)
            })
            .unwrap_or("unknown");

        let reply_text = if let Some(text) = reply.get("text").and_then(serde_json::Value::as_str) {
            text.to_string()
        } else if reply.get("voice").is_some() || reply.get("audio").is_some() {
            let reply_mid = reply.get("message_id").and_then(serde_json::Value::as_i64);
            let chat_id = message
                .get("chat")
                .and_then(|c| c.get("id"))
                .and_then(serde_json::Value::as_i64);
            if let (Some(mid), Some(cid)) = (reply_mid, chat_id) {
                self.voice_transcriptions
                    .lock()
                    .get(&format!("{cid}:{mid}"))
                    .map(|t| format!("[Voice] {t}"))
                    .unwrap_or_else(|| "[Voice message]".to_string())
            } else {
                "[Voice message]".to_string()
            }
        } else if reply.get("photo").is_some() {
            "[Photo]".to_string()
        } else if reply.get("document").is_some() {
            "[Document]".to_string()
        } else if reply.get("video").is_some() {
            "[Video]".to_string()
        } else if reply.get("sticker").is_some() {
            "[Sticker]".to_string()
        } else {
            "[Message]".to_string()
        };

        let quoted_lines: String = reply_text
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        Some(format!("> @{reply_sender}:\n{quoted_lines}"))
    }

    fn parse_update_message(&self, update: &serde_json::Value) -> Option<ChannelMessage> {
        let message = update.get("message")?;

        let text = message.get("text").and_then(serde_json::Value::as_str)?;

        let (username, sender_id, sender_identity) = Self::extract_sender_info(message);

        let mut identities = vec![username.as_str()];
        if let Some(id) = sender_id.as_deref() {
            identities.push(id);
        }

        if !self.is_any_user_allowed(identities.iter().copied()) {
            return None;
        }

        let is_group = Self::is_group_message(message);
        if self.mention_only && is_group {
            let bot_username = self.bot_username.lock();
            if let Some(ref bot_username) = *bot_username {
                if !Self::contains_bot_mention(text, bot_username) {
                    return None;
                }
            } else {
                return None;
            }
        }

        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string())?;

        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        let thread_id = message
            .get("message_thread_id")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());

        let reply_target = if let Some(ref tid) = thread_id {
            format!("{}:{}", chat_id, tid)
        } else {
            chat_id.clone()
        };

        let content = if self.mention_only && is_group {
            let bot_username = self.bot_username.lock();
            let bot_username = bot_username.as_ref()?;
            Self::normalize_incoming_content(text, bot_username)?
        } else {
            text.to_string()
        };

        let content = if let Some(quote) = self.extract_reply_context(message) {
            format!("{quote}\n\n{content}")
        } else {
            content
        };

        let content = if let Some(attr) = Self::format_forward_attribution(message) {
            format!("{attr}{content}")
        } else {
            content
        };

        if let Ok(mut vc) = self.voice_chats.lock() {
            vc.remove(&reply_target);
        }

        Some(ChannelMessage {
            id: format!("telegram_{chat_id}_{message_id}"),
            sender: sender_identity,
            reply_target,
            content,
            channel: "telegram".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: thread_id,
            interruption_scope_id: None,
            attachments: vec![],
        })
    }

    fn markdown_to_telegram_html(text: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let mut result_lines: Vec<String> = Vec::new();

        for line in &lines {
            let trimmed_line = line.trim_start();
            if trimmed_line.starts_with("```") {

                result_lines.push(trimmed_line.to_string());
                continue;
            }

            let mut line_out = String::new();

            let stripped = line.trim_start_matches('#');
            let header_level = line.len() - stripped.len();
            if header_level > 0 && line.starts_with('#') && stripped.starts_with(' ') {
                let title = Self::escape_html(stripped.trim());
                result_lines.push(format!("<b>{title}</b>"));
                continue;
            }

            let mut i = 0;
            let bytes = line.as_bytes();
            let len = bytes.len();
            while i < len {

                if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'*' {
                    if let Some(end) = line[i + 2..].find("**") {
                        let inner = Self::escape_html(&line[i + 2..i + 2 + end]);
                        let _ = write!(line_out, "<b>{inner}</b>");
                        i += 4 + end;
                        continue;
                    }
                }
                if i + 1 < len && bytes[i] == b'_' && bytes[i + 1] == b'_' {
                    if let Some(end) = line[i + 2..].find("__") {
                        let inner = Self::escape_html(&line[i + 2..i + 2 + end]);
                        let _ = write!(line_out, "<b>{inner}</b>");
                        i += 4 + end;
                        continue;
                    }
                }

                if bytes[i] == b'*' && (i == 0 || bytes[i - 1] != b'*') {
                    if let Some(end) = line[i + 1..].find('*') {
                        if end > 0 {
                            let inner = Self::escape_html(&line[i + 1..i + 1 + end]);
                            let _ = write!(line_out, "<i>{inner}</i>");
                            i += 2 + end;
                            continue;
                        }
                    }
                }

                if bytes[i] == b'`' && (i == 0 || bytes[i - 1] != b'`') {
                    if let Some(end) = line[i + 1..].find('`') {
                        let inner = Self::escape_html(&line[i + 1..i + 1 + end]);
                        let _ = write!(line_out, "<code>{inner}</code>");
                        i += 2 + end;
                        continue;
                    }
                }

                if bytes[i] == b'[' {
                    if let Some(bracket_end) = line[i + 1..].find(']') {
                        let text_part = &line[i + 1..i + 1 + bracket_end];
                        let after_bracket = i + 1 + bracket_end + 1;
                        if after_bracket < len && bytes[after_bracket] == b'(' {
                            if let Some(paren_end) = line[after_bracket + 1..].find(')') {
                                let url = &line[after_bracket + 1..after_bracket + 1 + paren_end];
                                if url.starts_with("http://") || url.starts_with("https://") {
                                    let text_html = Self::escape_html(text_part);
                                    let url_html = Self::escape_html(url);
                                    let _ =
                                        write!(line_out, "<a href=\"{url_html}\">{text_html}</a>");
                                    i = after_bracket + 1 + paren_end + 1;
                                    continue;
                                }
                            }
                        }
                    }
                }

                if i + 1 < len && bytes[i] == b'~' && bytes[i + 1] == b'~' {
                    if let Some(end) = line[i + 2..].find("~~") {
                        let inner = Self::escape_html(&line[i + 2..i + 2 + end]);
                        let _ = write!(line_out, "<s>{inner}</s>");
                        i += 4 + end;
                        continue;
                    }
                }

                let ch = line[i..].chars().next().unwrap();
                match ch {
                    '<' => line_out.push_str("&lt;"),
                    '>' => line_out.push_str("&gt;"),
                    '&' => line_out.push_str("&amp;"),
                    '"' => line_out.push_str("&quot;"),
                    '\'' => line_out.push_str("&#39;"),
                    _ => line_out.push(ch),
                }
                i += ch.len_utf8();
            }
            result_lines.push(line_out);
        }

        let joined = result_lines.join("\n");
        let mut final_out = String::with_capacity(joined.len());
        let mut in_code_block = false;
        let mut code_buf = String::new();

        for line in joined.split('\n') {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                    let escaped = code_buf.trim_end_matches('\n');

                    let _ = writeln!(final_out, "<pre><code>{escaped}</code></pre>");
                    code_buf.clear();
                } else {
                    in_code_block = true;
                    code_buf.clear();
                }
            } else if in_code_block {
                code_buf.push_str(line);
                code_buf.push('\n');
            } else {
                final_out.push_str(line);
                final_out.push('\n');
            }
        }
        if in_code_block && !code_buf.is_empty() {
            let _ = writeln!(final_out, "<pre><code>{}</code></pre>", code_buf.trim_end());
        }

        final_out.trim_end_matches('\n').to_string()
    }

    fn escape_html(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    async fn send_text_chunks(
        &self,
        message: &str,
        chat_id: &str,
        thread_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let chunks = split_message_for_telegram(message);

        for (index, chunk) in chunks.iter().enumerate() {
            let text = if chunks.len() > 1 {
                if index == 0 {
                    format!("{chunk}\n\n(continues...)")
                } else if index == chunks.len() - 1 {
                    format!("(continued)\n\n{chunk}")
                } else {
                    format!("(continued)\n\n{chunk}\n\n(continues...)")
                }
            } else {
                chunk.to_string()
            };

            let mut markdown_body = serde_json::json!({
                "chat_id": chat_id,
                "text": Self::markdown_to_telegram_html(&text),
                "parse_mode": "HTML"
            });

            if let Some(tid) = thread_id {
                markdown_body["message_thread_id"] = serde_json::Value::String(tid.to_string());
            }

            let markdown_resp = self
                .http_client()
                .post(self.api_url("sendMessage"))
                .json(&markdown_body)
                .send()
                .await?;

            if markdown_resp.status().is_success() {
                if index < chunks.len() - 1 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                continue;
            }

            let markdown_status = markdown_resp.status();
            let markdown_err = markdown_resp.text().await.unwrap_or_default();
            tracing::warn!(
                status = ?markdown_status,
                "Telegram sendMessage with Markdown failed; retrying without parse_mode"
            );

            let mut plain_body = serde_json::json!({
                "chat_id": chat_id,
                "text": text,
            });

            if let Some(tid) = thread_id {
                plain_body["message_thread_id"] = serde_json::Value::String(tid.to_string());
            }
            let plain_resp = self
                .http_client()
                .post(self.api_url("sendMessage"))
                .json(&plain_body)
                .send()
                .await?;

            if !plain_resp.status().is_success() {
                let plain_status = plain_resp.status();
                let plain_err = plain_resp.text().await.unwrap_or_default();
                anyhow::bail!(
                    "Telegram sendMessage failed (markdown {}: {}; plain {}: {})",
                    markdown_status,
                    markdown_err,
                    plain_status,
                    plain_err
                );
            }

            if index < chunks.len() - 1 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        Ok(())
    }

    async fn send_media_by_url(
        &self,
        method: &str,
        media_field: &str,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
        });
        body[media_field] = serde_json::Value::String(url.to_string());

        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url(method))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram {method} by URL failed: {err}");
        }

        tracing::info!("Telegram {method} sent to {chat_id}: {url}");
        Ok(())
    }

    async fn send_attachment(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        attachment: &TelegramAttachment,
    ) -> anyhow::Result<()> {
        let target = attachment.target.trim();

        if is_http_url(target) {
            let result = match attachment.kind {
                TelegramAttachmentKind::Image => {
                    self.send_photo_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Document => {
                    self.send_document_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Video => {
                    self.send_video_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Audio => {
                    self.send_audio_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Voice => {
                    self.send_voice_by_url(chat_id, thread_id, target, None)
                        .await
                }
            };

            if let Err(e) = result {
                tracing::warn!(
                    url = target,
                    error = %e,
                    "Telegram send media by URL failed; falling back to text link"
                );
                let kind_label = match attachment.kind {
                    TelegramAttachmentKind::Image => "Image",
                    TelegramAttachmentKind::Document => "Document",
                    TelegramAttachmentKind::Video => "Video",
                    TelegramAttachmentKind::Audio => "Audio",
                    TelegramAttachmentKind::Voice => "Voice",
                };
                let fallback_text = format!("{kind_label}: {target}");
                self.send_text_chunks(&fallback_text, chat_id, thread_id)
                    .await?;
            }

            return Ok(());
        }

        let remapped;
        let target = if let Some(rel) = target.strip_prefix("/workspace/") {
            if let Some(ws) = &self.workspace_dir {
                remapped = ws.join(rel);
                remapped.to_str().unwrap_or(target)
            } else {
                target
            }
        } else {
            target
        };

        let path = Path::new(target);
        if !path.exists() {
            anyhow::bail!("Telegram attachment path not found: {target}");
        }

        match attachment.kind {
            TelegramAttachmentKind::Image => self.send_photo(chat_id, thread_id, path, None).await,
            TelegramAttachmentKind::Document => {
                self.send_document(chat_id, thread_id, path, None).await
            }
            TelegramAttachmentKind::Video => self.send_video(chat_id, thread_id, path, None).await,
            TelegramAttachmentKind::Audio => self.send_audio(chat_id, thread_id, path, None).await,
            TelegramAttachmentKind::Voice => self.send_voice(chat_id, thread_id, path, None).await,
        }
    }

    pub async fn send_document(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendDocument"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument failed: {err}");
        }

        tracing::info!("Telegram document sent to {chat_id}: {file_name}");
        Ok(())
    }

    pub async fn send_document_bytes(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_bytes: Vec<u8>,
        file_name: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendDocument"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument failed: {err}");
        }

        tracing::info!("Telegram document sent to {chat_id}: {file_name}");
        Ok(())
    }

    pub async fn send_photo(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("photo.jpg");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendPhoto"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto failed: {err}");
        }

        tracing::info!("Telegram photo sent to {chat_id}: {file_name}");
        Ok(())
    }

    pub async fn send_photo_bytes(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_bytes: Vec<u8>,
        file_name: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendPhoto"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto failed: {err}");
        }

        tracing::info!("Telegram photo sent to {chat_id}: {file_name}");
        Ok(())
    }

    pub async fn send_video(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video.mp4");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("video", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendVideo"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendVideo failed: {err}");
        }

        tracing::info!("Telegram video sent to {chat_id}: {file_name}");
        Ok(())
    }

    pub async fn send_audio(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.mp3");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("audio", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendAudio"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendAudio failed: {err}");
        }

        tracing::info!("Telegram audio sent to {chat_id}: {file_name}");
        Ok(())
    }

    pub async fn send_voice(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("voice.ogg");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendVoice"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendVoice failed: {err}");
        }

        tracing::info!("Telegram voice sent to {chat_id}: {file_name}");
        Ok(())
    }

    pub async fn send_document_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "document": url
        });

        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendDocument"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument by URL failed: {err}");
        }

        tracing::info!("Telegram document (URL) sent to {chat_id}: {url}");
        Ok(())
    }

    pub async fn send_photo_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "photo": url
        });

        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendPhoto"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto by URL failed: {err}");
        }

        tracing::info!("Telegram photo (URL) sent to {chat_id}: {url}");
        Ok(())
    }

    pub async fn send_video_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_media_by_url("sendVideo", "video", chat_id, thread_id, url, caption)
            .await
    }

    pub async fn send_audio_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_media_by_url("sendAudio", "audio", chat_id, thread_id, url, caption)
            .await
    }

    pub async fn send_voice_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_media_by_url("sendVoice", "voice", chat_id, thread_id, url, caption)
            .await
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn supports_draft_updates(&self) -> bool {
        self.stream_mode != StreamMode::Off
    }

    async fn send_draft(&self, message: &SendMessage) -> anyhow::Result<Option<String>> {
        if self.stream_mode == StreamMode::Off {
            return Ok(None);
        }

        let (chat_id, thread_id) = Self::parse_reply_target(&message.recipient);
        let initial_text = if message.content.is_empty() {
            "...".to_string()
        } else {
            message.content.clone()
        };

        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": initial_text,
        });
        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Telegram sendMessage (draft) failed: {err}");
        }

        let resp_json: serde_json::Value = resp.json().await?;
        let message_id = resp_json
            .get("result")
            .and_then(|r| r.get("message_id"))
            .and_then(|id| id.as_i64())
            .map(|id| id.to_string());

        self.last_draft_edit
            .lock()
            .insert(chat_id.to_string(), std::time::Instant::now());

        Ok(message_id)
    }

    async fn update_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let (chat_id, _) = Self::parse_reply_target(recipient);

        {
            let mut last_edits = self.last_draft_edit.lock();

            let ttl = std::time::Duration::from_secs(3600);
            last_edits.retain(|_, ts| ts.elapsed() < ttl);

            if let Some(last_time) = last_edits.get(&chat_id) {
                let elapsed = u64::try_from(last_time.elapsed().as_millis()).unwrap_or(u64::MAX);
                if elapsed < self.draft_update_interval_ms {
                    return Ok(());
                }
            }
        }

        let display_text = if text.len() > TELEGRAM_MAX_MESSAGE_LENGTH {
            let mut end = 0;
            for (idx, ch) in text.char_indices() {
                let next = idx + ch.len_utf8();
                if next > TELEGRAM_MAX_MESSAGE_LENGTH {
                    break;
                }
                end = next;
            }
            &text[..end]
        } else {
            text
        };

        let message_id_parsed = match message_id.parse::<i64>() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("Invalid Telegram message_id '{message_id}': {e}");
                return Ok(());
            }
        };

        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id_parsed,
            "text": display_text,
        });

        let resp = self
            .client
            .post(self.api_url("editMessageText"))
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            self.last_draft_edit
                .lock()
                .insert(chat_id.clone(), std::time::Instant::now());
        } else {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            tracing::debug!("Telegram editMessageText failed ({status}): {err}");
        }

        Ok(())
    }

    async fn finalize_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let text = &strip_tool_call_tags(text);
        let (chat_id, thread_id) = Self::parse_reply_target(recipient);

        self.last_draft_edit.lock().remove(&chat_id);

        let (text_without_markers, attachments) = parse_attachment_markers(text);

        let msg_id = match message_id.parse::<i64>() {
            Ok(id) => Some(id),
            Err(e) => {
                tracing::warn!("Invalid Telegram message_id '{message_id}': {e}");
                None
            }
        };

        if !attachments.is_empty() {

            if let Some(id) = msg_id {
                let _ = self
                    .client
                    .post(self.api_url("deleteMessage"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": id,
                    }))
                    .send()
                    .await;
            }

            if !text_without_markers.is_empty() {
                self.send_text_chunks(&text_without_markers, &chat_id, thread_id.as_deref())
                    .await?;
            }

            for attachment in &attachments {
                self.send_attachment(&chat_id, thread_id.as_deref(), attachment)
                    .await?;
            }

            return Ok(());
        }

        if text.len() > TELEGRAM_MAX_MESSAGE_LENGTH {
            if let Some(id) = msg_id {
                let _ = self
                    .client
                    .post(self.api_url("deleteMessage"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": id,
                    }))
                    .send()
                    .await;
            }

            return self
                .send_text_chunks(text, &chat_id, thread_id.as_deref())
                .await;
        }

        let Some(id) = msg_id else {
            return self
                .send_text_chunks(text, &chat_id, thread_id.as_deref())
                .await;
        };

        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": id,
            "text": Self::markdown_to_telegram_html(text),
            "parse_mode": "HTML",
        });

        let resp = self
            .client
            .post(self.api_url("editMessageText"))
            .json(&body)
            .send()
            .await?;

        match Self::classify_edit_message_response(resp).await {
            EditMessageResult::Success | EditMessageResult::NotModified => return Ok(()),
            EditMessageResult::Failed(status) => {
                tracing::debug!(
                    status = ?status,
                    "Telegram finalize_draft HTML edit failed; retrying without parse_mode"
                );
            }
        }

        let plain_body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": id,
            "text": text,
        });

        let resp = self
            .client
            .post(self.api_url("editMessageText"))
            .json(&plain_body)
            .send()
            .await?;

        match Self::classify_edit_message_response(resp).await {
            EditMessageResult::Success | EditMessageResult::NotModified => return Ok(()),
            EditMessageResult::Failed(status) => {
                tracing::warn!(
                    status = ?status,
                    "Telegram finalize_draft plain edit failed; attempting delete+send fallback"
                );
            }
        }

        let delete_resp = self
            .client
            .post(self.api_url("deleteMessage"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": id,
            }))
            .send()
            .await;

        match delete_resp {
            Ok(resp) if resp.status().is_success() => {
                self.send_text_chunks(text, &chat_id, thread_id.as_deref())
                    .await
            }
            Ok(resp) => {
                tracing::warn!(
                    status = ?resp.status(),
                    "Telegram finalize_draft delete failed; skipping sendMessage to avoid duplicate"
                );
                Ok(())
            }
            Err(err) => {
                tracing::warn!(
                    "Telegram finalize_draft delete request failed: {err}; skipping sendMessage to avoid duplicate"
                );
                Ok(())
            }
        }
    }

    async fn cancel_draft(&self, recipient: &str, message_id: &str) -> anyhow::Result<()> {
        let (chat_id, _) = Self::parse_reply_target(recipient);
        self.last_draft_edit.lock().remove(&chat_id);

        let message_id = match message_id.parse::<i64>() {
            Ok(id) => id,
            Err(e) => {
                tracing::debug!("Invalid Telegram draft message_id '{message_id}': {e}");
                return Ok(());
            }
        };

        let response = self
            .client
            .post(self.api_url("deleteMessage"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::debug!("Telegram deleteMessage failed ({status}): {body}");
        }

        Ok(())
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        let content = strip_tool_call_tags(&message.content);

        let (chat_id, thread_id) = match message.recipient.split_once(':') {
            Some((chat, thread)) => (chat, Some(thread)),
            None => (message.recipient.as_str(), None),
        };

        let is_voice_chat = self
            .voice_chats
            .lock()
            .map(|vs| vs.contains(&message.recipient))
            .unwrap_or(false);

        if is_voice_chat && self.tts_config.is_some() {

            let is_substantive = content.len() > 40
                && !content.starts_with("http")
                && !content.starts_with('{')
                && !content.starts_with('[')
                && !content.starts_with("Error")
                && !content.contains("```")
                && !content.contains("tool_call")
                && !content.contains("wttr.in");

            if is_substantive {
                if let Ok(mut pv) = self.pending_voice.lock() {
                    pv.insert(
                        message.recipient.clone(),
                        (content.clone(), std::time::Instant::now()),
                    );
                }

                let pending = self.pending_voice.clone();
                let voice_chats = self.voice_chats.clone();
                let api_base = self.api_base.clone();
                let bot_token = self.bot_token.clone();
                let chat_id_owned = chat_id.to_string();
                let thread_id_owned = thread_id.map(str::to_string);
                let recipient = message.recipient.clone();
                if let Some(tts_config) = self.tts_config.clone() {
                    let _voice_task = crate::runtime::spawn_supervised(
                        "channels.telegram.voice_tts",
                        async move {

                            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

                            let to_voice = pending.lock().ok().and_then(|mut pv| {
                                if let Some((_, ts)) = pv.get(&recipient) {
                                    if ts.elapsed().as_secs() >= 8 {
                                        return pv.remove(&recipient).map(|(text, _)| text);
                                    }
                                }
                                None
                            });

                            if let Some(text) = to_voice {
                                if let Ok(mut vc) = voice_chats.lock() {
                                    vc.remove(&recipient);
                                }
                                match Self::synthesize_and_send_voice(
                                    &api_base,
                                    &bot_token,
                                    &chat_id_owned,
                                    thread_id_owned.as_deref(),
                                    &text,
                                    &tts_config,
                                )
                                .await
                                {
                                    Ok(()) => {
                                        tracing::info!(
                                            "Telegram: voice reply sent ({} chars)",
                                            text.len()
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!("Telegram: TTS voice reply failed: {e}");
                                    }
                                }
                            }
                        },
                    );
                }
            }
        }

        let (text_without_markers, attachments) = parse_attachment_markers(&content);

        if !attachments.is_empty() {
            if !text_without_markers.is_empty() {
                self.send_text_chunks(&text_without_markers, chat_id, thread_id)
                    .await?;
            }

            for attachment in &attachments {
                self.send_attachment(chat_id, thread_id, attachment).await?;
            }

            return Ok(());
        }

        if let Some(attachment) = parse_path_only_attachment(&content) {
            self.send_attachment(chat_id, thread_id, &attachment)
                .await?;
            return Ok(());
        }

        self.send_text_chunks(&content, chat_id, thread_id).await
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut offset: i64 = 0;

        if self.mention_only {
            let _ = self.get_bot_username().await;
        }

        tracing::info!("Telegram channel listening for messages...");

        loop {
            let url = self.api_url("getUpdates");
            let probe = serde_json::json!({
                "offset": offset,
                "timeout": 0,
                "allowed_updates": ["message"]
            });
            match self.http_client().post(&url).json(&probe).send().await {
                Err(e) => {
                    tracing::warn!("Telegram startup probe error: {e}; retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Ok(resp) => {
                    match resp.json::<serde_json::Value>().await {
                        Err(e) => {
                            tracing::warn!(
                                "Telegram startup probe parse error: {e}; retrying in 5s"
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                        Ok(data) => {
                            let ok = data
                                .get("ok")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            if ok {

                                if let Some(results) =
                                    data.get("result").and_then(serde_json::Value::as_array)
                                {
                                    for update in results {
                                        if let Some(uid) = update
                                            .get("update_id")
                                            .and_then(serde_json::Value::as_i64)
                                        {
                                            offset = uid + 1;
                                        }
                                    }
                                }
                                break;
                            }

                            let error_code = data
                                .get("error_code")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or_default();
                            if error_code == 409 {
                                tracing::debug!("Startup probe: slot busy (409), retrying in 5s");
                            } else {
                                let desc = data
                                    .get("description")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("unknown");
                                tracing::warn!(
                                    "Startup probe: API error {error_code}: {desc}; retrying in 5s"
                                );
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        }

        tracing::debug!("Startup probe succeeded; entering main long-poll loop.");

        loop {
            if self.mention_only {
                let missing_username = self.bot_username.lock().is_none();
                if missing_username {
                    let _ = self.get_bot_username().await;
                }
            }

            let url = self.api_url("getUpdates");
            let body = serde_json::json!({
                "offset": offset,
                "timeout": 30,
                "allowed_updates": ["message"]
            });

            let resp = match self.http_client().post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Telegram poll error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Telegram parse error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let ok = data
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            if !ok {
                let error_code = data
                    .get("error_code")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or_default();
                let description = data
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown Telegram API error");

                if error_code == 409 {
                    tracing::warn!(
                        "Telegram polling conflict (409): {description}. \
Ensure only one `sen` process is using this bot token."
                    );

                    tokio::time::sleep(std::time::Duration::from_secs(35)).await;
                } else {
                    tracing::warn!(
                        "Telegram getUpdates API error (code={}): {description}",
                        error_code
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                continue;
            }

            if let Some(results) = data.get("result").and_then(serde_json::Value::as_array) {
                for update in results {

                    if let Some(uid) = update.get("update_id").and_then(serde_json::Value::as_i64) {
                        offset = uid + 1;
                    }

                    let msg = if let Some(m) = self.parse_update_message(update) {
                        m
                    } else if let Some(m) = self.try_parse_voice_message(update).await {
                        m
                    } else if let Some(m) = self.try_parse_attachment_message(update).await {
                        m
                    } else {
                        Box::pin(self.handle_unauthorized_message(update)).await;
                        continue;
                    };

                    if self.ack_reactions {
                        if let Some((reaction_chat_id, reaction_message_id)) =
                            Self::extract_update_message_target(update)
                        {
                            self.try_add_ack_reaction_nonblocking(
                                reaction_chat_id,
                                reaction_message_id,
                            );
                        }
                    }

                    let typing_body = serde_json::json!({
                        "chat_id": &msg.reply_target,
                        "action": "typing"
                    });
                    let _ = self
                        .http_client()
                        .post(self.api_url("sendChatAction"))
                        .json(&typing_body)
                        .send()
                        .await;

                    if tx.send(msg).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        let timeout_duration = Duration::from_secs(5);

        match tokio::time::timeout(
            timeout_duration,
            self.http_client().get(self.api_url("getMe")).send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp.status().is_success(),
            Ok(Err(e)) => {
                tracing::debug!("Telegram health check failed: {e}");
                false
            }
            Err(_) => {
                tracing::debug!("Telegram health check timed out after 5s");
                false
            }
        }
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        self.stop_typing(recipient).await?;

        let client = self.http_client();
        let url = self.api_url("sendChatAction");
        let chat_id = recipient.to_string();

        let handle =
            crate::runtime::spawn_supervised("channels.telegram.typing_indicator", async move {
                loop {
                    let body = serde_json::json!({
                        "chat_id": &chat_id,
                        "action": "typing"
                    });
                    let _ = client.post(&url).json(&body).send().await;

                    tokio::time::sleep(Duration::from_secs(4)).await;
                }
            });

        let mut guard = self.typing_handle.lock();
        *guard = Some(handle);

        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        let mut guard = self.typing_handle.lock();
        if let Some(handle) = guard.take() {
            handle.abort();
        }
        Ok(())
    }
}
