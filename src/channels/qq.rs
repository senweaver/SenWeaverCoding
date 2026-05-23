// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_AUTH_URL: &str = "https://bots.qq.com/app/getAppAccessToken";

const QQ_MAX_UPLOAD_BYTES: u64 = 10 * 1024 * 1024;

const UPLOAD_CACHE_CAPACITY: usize = 500;

const REPLY_LIMIT: u32 = 4;

const REPLY_TTL_SECS: u64 = 3600;

const REPLY_TRACKER_CAPACITY: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QQMediaFileType {

    Image = 1,

    Video = 2,

    Voice = 3,

    File = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QQMediaAttachment {
    kind: QQMediaFileType,
    target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QQSendSegment {
    Text(String),
    Media(QQMediaAttachment),
}

#[derive(Debug, Deserialize)]
struct QQUploadResponse {
    file_info: String,
    #[allow(dead_code)]
    file_uuid: Option<String>,
    ttl: Option<u64>,
}

struct UploadCacheEntry {
    file_info: String,
    expires_at: u64,
}

struct ReplyRecord {
    count: u32,
    first_reply_at: u64,
}

fn ensure_https(url: &str) -> anyhow::Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!(
            "Refusing to transmit sensitive data over non-HTTPS URL: URL scheme must be https"
        );
    }
    Ok(())
}

fn is_native_voice_ext(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "wav" | "mp3" | "silk")
}

fn marker_kind_to_qq_file_type(marker: &str, target: &str) -> Option<QQMediaFileType> {
    match marker.trim().to_ascii_uppercase().as_str() {
        "IMAGE" | "PHOTO" => Some(QQMediaFileType::Image),
        "DOCUMENT" | "FILE" => Some(QQMediaFileType::File),
        "VIDEO" => Some(QQMediaFileType::Video),
        "AUDIO" | "VOICE" => {
            let ext = Path::new(target.split('?').next().unwrap_or(target))
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if is_native_voice_ext(ext) {
                Some(QQMediaFileType::Voice)
            } else {
                Some(QQMediaFileType::File)
            }
        }
        _ => None,
    }
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

fn parse_qq_attachment_markers(content: &str) -> (String, Vec<QQMediaAttachment>) {
    let mut cleaned = String::with_capacity(content.len());
    let mut attachments = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some(open_rel) = content[cursor..].find('[') else {
            cleaned.push_str(&content[cursor..]);
            break;
        };

        let open = cursor + open_rel;
        cleaned.push_str(&content[cursor..open]);

        let Some(close_rel) = find_matching_close(&content[open + 1..]) else {
            cleaned.push_str(&content[open..]);
            break;
        };

        let close = open + 1 + close_rel;
        let marker = &content[open + 1..close];

        let parsed = marker.split_once(':').and_then(|(kind, target)| {
            let target = target.trim();
            if target.is_empty() {
                return None;
            }
            let file_type = marker_kind_to_qq_file_type(kind, target)?;
            Some(QQMediaAttachment {
                kind: file_type,
                target: target.to_string(),
            })
        });

        if let Some(attachment) = parsed {
            attachments.push(attachment);
        } else {
            cleaned.push_str(&content[open..=close]);
        }

        cursor = close + 1;
    }

    (cleaned.trim().to_string(), attachments)
}

fn infer_attachment_marker(content_type: &str, filename: &str) -> &'static str {
    let ct = content_type.to_ascii_lowercase();
    if ct.starts_with("image/") {
        return "IMAGE";
    }
    if ct.starts_with("audio/") || ct.contains("voice") {
        return "VOICE";
    }
    if ct.starts_with("video/") {
        return "VIDEO";
    }

    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".heic")
        || lower.ends_with(".heif")
        || lower.ends_with(".svg")
    {
        return "IMAGE";
    }
    if lower.ends_with(".mp3")
        || lower.ends_with(".wav")
        || lower.ends_with(".silk")
        || lower.ends_with(".ogg")
        || lower.ends_with(".flac")
        || lower.ends_with(".m4a")
    {
        return "VOICE";
    }
    if lower.ends_with(".mp4")
        || lower.ends_with(".mov")
        || lower.ends_with(".mkv")
        || lower.ends_with(".avi")
        || lower.ends_with(".webm")
    {
        return "VIDEO";
    }
    "DOCUMENT"
}

fn fix_qq_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        trimmed.to_string()
    }
}

fn next_msg_seq() -> u32 {
    #[allow(clippy::cast_possible_truncation)]
    let time_part = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32)
        % 100_000_000;
    let random = u32::from(rand::random::<u16>());
    (time_part ^ random) % 65536
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

const DEDUP_CAPACITY: usize = 10_000;

const AUTH_RETRY_MAX_ATTEMPTS: u32 = 4;

const AUTH_RETRY_INITIAL_BACKOFF_MS: u64 = 500;

const AUTH_RETRY_MAX_BACKOFF_MS: u64 = 8_000;

pub struct QQChannel {
    app_id: String,
    app_secret: String,
    allowed_users: Vec<String>,

    token_cache: Arc<RwLock<Option<(String, u64)>>>,

    dedup: Arc<RwLock<HashSet<String>>>,

    workspace_dir: Option<PathBuf>,

    upload_cache: Arc<RwLock<HashMap<String, UploadCacheEntry>>>,

    reply_tracker: Arc<RwLock<HashMap<String, ReplyRecord>>>,

    proxy_url: Option<String>,

    session_id: Arc<RwLock<Option<String>>>,

    last_sequence: Arc<RwLock<Option<i64>>>,
}

impl QQChannel {
    pub fn new(app_id: String, app_secret: String, allowed_users: Vec<String>) -> Self {
        Self {
            app_id,
            app_secret,
            allowed_users,
            token_cache: Arc::new(RwLock::new(None)),
            dedup: Arc::new(RwLock::new(HashSet::new())),
            workspace_dir: None,
            upload_cache: Arc::new(RwLock::new(HashMap::new())),
            reply_tracker: Arc::new(RwLock::new(HashMap::new())),
            proxy_url: None,
            session_id: Arc::new(RwLock::new(None)),
            last_sequence: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_workspace_dir(mut self, dir: PathBuf) -> Self {
        self.workspace_dir = Some(dir);
        self
    }

    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::get_services()
            .proxy_runtime()
            .build_channel_client("channel.qq", self.proxy_url.as_deref())
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    async fn fetch_access_token(&self) -> anyhow::Result<(String, u64)> {
        let body = json!({
            "appId": self.app_id,
            "clientSecret": self.app_secret,
        });

        let resp = self
            .http_client()
            .post(QQ_AUTH_URL)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("QQ token request failed ({status}): {err}");
        }

        let data: serde_json::Value = resp.json().await?;
        let token = data
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing access_token in QQ response"))?
            .to_string();

        let expires_in = data
            .get("expires_in")
            .and_then(|e| e.as_str())
            .and_then(|e| e.parse::<u64>().ok())
            .unwrap_or(7200);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let expiry = now + expires_in.saturating_sub(60);

        Ok((token, expiry))
    }

    async fn fetch_access_token_with_retry(&self) -> anyhow::Result<(String, u64)> {
        let mut backoff_ms = AUTH_RETRY_INITIAL_BACKOFF_MS;
        let mut last_err = None;

        for attempt in 1..=AUTH_RETRY_MAX_ATTEMPTS {
            match self.fetch_access_token().await {
                Ok(result) => {
                    if attempt > 1 {
                        tracing::info!(
                            "QQ: getAppAccessToken succeeded on attempt {attempt}/{AUTH_RETRY_MAX_ATTEMPTS}"
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    tracing::warn!(
                        "QQ: getAppAccessToken failed (attempt {attempt}/{AUTH_RETRY_MAX_ATTEMPTS}): {e}"
                    );
                    last_err = Some(e);

                    if attempt < AUTH_RETRY_MAX_ATTEMPTS {

                        let jitter_factor = 0.75 + (rand::random::<f64>() * 0.5);
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let sleep_ms = (backoff_ms as f64 * jitter_factor) as u64;
                        tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                        backoff_ms = (backoff_ms * 2).min(AUTH_RETRY_MAX_BACKOFF_MS);
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("QQ: getAppAccessToken failed after {AUTH_RETRY_MAX_ATTEMPTS} attempts")
        }))
    }

    async fn get_token(&self) -> anyhow::Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        {
            let cache = self.token_cache.read().await;
            if let Some((ref token, expiry)) = *cache {
                if now < expiry {
                    return Ok(token.clone());
                }
            }
        }

        let (token, expiry) = self.fetch_access_token_with_retry().await?;
        {
            let mut cache = self.token_cache.write().await;
            *cache = Some((token.clone(), expiry));
        }
        Ok(token)
    }

    async fn get_gateway_url(&self, token: &str) -> anyhow::Result<String> {
        let resp = self
            .http_client()
            .get(format!("{QQ_API_BASE}/gateway"))
            .header("Authorization", format!("QQBot {token}"))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("QQ gateway request failed ({status}): {err}");
        }

        let data: serde_json::Value = resp.json().await?;
        let url = data
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing gateway URL in QQ response"))?
            .to_string();

        Ok(url)
    }

    async fn is_duplicate(&self, msg_id: &str) -> bool {
        if msg_id.is_empty() {
            return false;
        }

        let mut dedup = self.dedup.write().await;

        if dedup.contains(msg_id) {
            return true;
        }

        if dedup.len() >= DEDUP_CAPACITY {
            let to_remove: Vec<String> = dedup.iter().take(DEDUP_CAPACITY / 2).cloned().collect();
            for key in to_remove {
                dedup.remove(&key);
            }
        }

        dedup.insert(msg_id.to_string());
        false
    }

    fn upload_cache_key(
        file_data: &[u8],
        scope: &str,
        target_id: &str,
        file_type: QQMediaFileType,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(file_data);
        let hash = format!("{:x}", hasher.finalize());
        format!("{hash}:{scope}:{target_id}:{}", file_type as u8)
    }

    async fn get_cached_upload(&self, cache_key: &str) -> Option<String> {
        let cache = self.upload_cache.read().await;
        if let Some(entry) = cache.get(cache_key) {

            if now_secs() + 60 < entry.expires_at {
                return Some(entry.file_info.clone());
            }
        }
        None
    }

    async fn set_cached_upload(&self, cache_key: String, file_info: String, ttl: u64) {
        let mut cache = self.upload_cache.write().await;

        if cache.len() >= UPLOAD_CACHE_CAPACITY {
            let now = now_secs();
            cache.retain(|_, v| v.expires_at > now);

            if cache.len() >= UPLOAD_CACHE_CAPACITY {
                let keys_to_remove: Vec<String> = cache
                    .keys()
                    .take(UPLOAD_CACHE_CAPACITY / 2)
                    .cloned()
                    .collect();
                for key in keys_to_remove {
                    cache.remove(&key);
                }
            }
        }

        cache.insert(
            cache_key,
            UploadCacheEntry {
                file_info,
                expires_at: now_secs() + ttl,
            },
        );
    }

    async fn check_reply_allowed(&self, msg_id: &str) -> bool {
        let now = now_secs();
        let mut tracker = self.reply_tracker.write().await;

        if tracker.len() >= REPLY_TRACKER_CAPACITY {
            tracker.retain(|_, v| now - v.first_reply_at < REPLY_TTL_SECS);
        }

        if let Some(record) = tracker.get_mut(msg_id) {
            if now - record.first_reply_at >= REPLY_TTL_SECS {

                return false;
            }
            if record.count >= REPLY_LIMIT {
                return false;
            }
            record.count += 1;
            true
        } else {
            tracker.insert(
                msg_id.to_string(),
                ReplyRecord {
                    count: 1,
                    first_reply_at: now,
                },
            );
            true
        }
    }

    fn resolve_recipient(recipient: &str) -> (&str, String) {
        if let Some(group_id) = recipient.strip_prefix("group:") {
            ("groups", group_id.to_string())
        } else {
            let raw_uid = recipient.strip_prefix("user:").unwrap_or(recipient);
            let user_id: String = raw_uid
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            ("users", user_id)
        }
    }

    async fn upload_media(
        &self,
        recipient: &str,
        file_type: QQMediaFileType,
        url: Option<&str>,
        file_data: Option<&str>,
        file_name: Option<&str>,
    ) -> anyhow::Result<(String, Option<u64>)> {
        let token = self.get_token().await?;
        let (scope, id) = Self::resolve_recipient(recipient);

        let api_url = format!("{QQ_API_BASE}/v2/{scope}/{id}/files");
        ensure_https(&api_url)?;

        let mut body = json!({
            "file_type": file_type as u8,
            "srv_send_msg": false,
        });

        if let Some(u) = url {
            body["url"] = json!(u);
        }
        if let Some(d) = file_data {
            body["file_data"] = json!(d);
        }

        if file_type == QQMediaFileType::File {
            if let Some(name) = file_name {
                body["file_name"] = json!(name);
            }
        }

        let resp = self
            .http_client()
            .post(&api_url)
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("QQ upload media failed ({status}): {err}");
        }

        let upload_resp: QQUploadResponse = resp.json().await?;
        Ok((upload_resp.file_info, upload_resp.ttl))
    }

    async fn send_media_message(&self, recipient: &str, file_info: &str) -> anyhow::Result<()> {
        let token = self.get_token().await?;
        let (scope, id) = Self::resolve_recipient(recipient);

        let url = format!("{QQ_API_BASE}/v2/{scope}/{id}/messages");
        ensure_https(&url)?;

        let body = json!({
            "msg_type": 7,
            "media": {
                "file_info": file_info,
            },
            "msg_seq": next_msg_seq(),
        });

        let resp = self
            .http_client()
            .post(&url)
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("QQ send media message failed ({status}): {err}");
        }

        Ok(())
    }

    async fn send_attachment(
        &self,
        recipient: &str,
        attachment: &QQMediaAttachment,
    ) -> anyhow::Result<()> {
        let target = attachment.target.trim();

        let file_name = Path::new(target.split('?').next().unwrap_or(target))
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());

        if target.starts_with("http://") || target.starts_with("https://") {

            let (file_info, _ttl) = self
                .upload_media(
                    recipient,
                    attachment.kind,
                    Some(target),
                    None,
                    file_name.as_deref(),
                )
                .await?;
            self.send_media_message(recipient, &file_info).await?;
        } else {

            let path = Path::new(target);
            if !path.exists() {
                anyhow::bail!("QQ attachment path not found: {target}");
            }

            let metadata = tokio::fs::metadata(path).await?;
            if metadata.len() > QQ_MAX_UPLOAD_BYTES {
                anyhow::bail!(
                    "QQ attachment too large ({} bytes, max {}): {target}",
                    metadata.len(),
                    QQ_MAX_UPLOAD_BYTES
                );
            }

            let file_bytes = tokio::fs::read(path).await?;
            let (scope_label, target_id) = Self::resolve_recipient(recipient);
            let scope = if scope_label == "groups" {
                "group"
            } else {
                "c2c"
            };
            let cache_key = Self::upload_cache_key(&file_bytes, scope, &target_id, attachment.kind);

            if let Some(cached_file_info) = self.get_cached_upload(&cache_key).await {
                tracing::debug!("QQ: using cached upload for {target}");
                self.send_media_message(recipient, &cached_file_info)
                    .await?;
                return Ok(());
            }

            let b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
            let (file_info, ttl) = self
                .upload_media(
                    recipient,
                    attachment.kind,
                    None,
                    Some(&b64),
                    file_name.as_deref(),
                )
                .await?;

            if let Some(ttl_secs) = ttl {
                self.set_cached_upload(cache_key, file_info.clone(), ttl_secs)
                    .await;
            }

            self.send_media_message(recipient, &file_info).await?;
        }

        Ok(())
    }

    async fn compose_message_content(&self, payload: &serde_json::Value) -> Option<String> {
        let text = payload
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .trim();

        let mut markers: Vec<String> = Vec::new();
        let mut voice_transcripts: Vec<String> = Vec::new();

        if let Some(attachments) = payload.get("attachments").and_then(|a| a.as_array()) {
            for att in attachments {
                let url = match att.get("url").and_then(|u| u.as_str()) {
                    Some(u) if !u.trim().is_empty() => fix_qq_url(u),
                    _ => continue,
                };

                let content_type = att
                    .get("content_type")
                    .and_then(|ct| ct.as_str())
                    .unwrap_or("");
                let filename = att
                    .get("filename")
                    .and_then(|f| f.as_str())
                    .unwrap_or("attachment");

                let marker_type = infer_attachment_marker(content_type, filename);

                let is_voice = content_type == "voice"
                    || content_type.starts_with("audio/")
                    || marker_type == "VOICE";
                let (download_url, save_filename) = if is_voice {
                    if let Some(wav_url) = att
                        .get("voice_wav_url")
                        .and_then(|u| u.as_str())
                        .filter(|u| !u.trim().is_empty())
                    {
                        let fixed = fix_qq_url(wav_url);

                        let wav_name = Path::new(fixed.split('?').next().unwrap_or(&fixed))
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("voice.wav")
                            .to_string();
                        (fixed, wav_name)
                    } else {
                        (url.clone(), filename.to_string())
                    }
                } else {
                    (url.clone(), filename.to_string())
                };

                let location = if let Some(ref ws) = self.workspace_dir {
                    let dir = ws.join("qq_files");
                    match self
                        .download_attachment(&download_url, &dir, &save_filename)
                        .await
                    {
                        Ok(local_path) => local_path.display().to_string(),
                        Err(e) => {
                            tracing::warn!("QQ: failed to download attachment: {e}");
                            url.clone()
                        }
                    }
                } else {
                    url.clone()
                };

                if is_voice {

                    markers.push(format!("[{marker_type}:{location}]"));
                    if let Some(asr_text) = att
                        .get("asr_refer_text")
                        .and_then(|t| t.as_str())
                        .map(|t| t.trim())
                        .filter(|t| !t.is_empty())
                    {
                        voice_transcripts.push(asr_text.to_string());
                    }
                } else {
                    markers.push(format!("[{marker_type}:{location}]"));
                }
            }
        }

        let voice_text = match voice_transcripts.len() {
            0 => String::new(),
            1 => format!(
                "<VOICE_TRANSCRIPTION>{}</VOICE_TRANSCRIPTION>",
                voice_transcripts[0]
            ),
            _ => voice_transcripts
                .iter()
                .enumerate()
                .map(|(i, t)| format!("<VOICE_TRANSCRIPTION_{i}>{t}</VOICE_TRANSCRIPTION_{i}>"))
                .collect::<Vec<_>>()
                .join("\n"),
        };

        let mut parts: Vec<&str> = Vec::new();
        if !text.is_empty() {
            parts.push(text);
        }
        if !voice_text.is_empty() {
            parts.push(&voice_text);
        }
        let markers_joined = markers.join("\n");
        if !markers_joined.is_empty() {
            parts.push(&markers_joined);
        }

        if parts.is_empty() {
            return None;
        }

        Some(parts.join("\n"))
    }

    async fn download_attachment(
        &self,
        url: &str,
        dir: &Path,
        filename: &str,
    ) -> anyhow::Result<PathBuf> {
        tokio::fs::create_dir_all(dir).await?;

        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let unique = &Uuid::new_v4().to_string()[..8];
        let safe_name = if ext.is_empty() {
            format!("{stem}_{unique}")
        } else {
            format!("{stem}_{unique}.{ext}")
        };

        let dest = dir.join(&safe_name);

        let resp = self.http_client().get(url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Download failed ({}): {url}", resp.status());
        }

        let bytes = resp.bytes().await?;
        tokio::fs::write(&dest, &bytes).await?;

        Ok(dest)
    }

    async fn send_text_markdown(&self, recipient: &str, content: &str) -> anyhow::Result<()> {
        let token = self.get_token().await?;
        let (scope, id) = Self::resolve_recipient(recipient);

        let url = format!("{QQ_API_BASE}/v2/{scope}/{id}/messages");
        ensure_https(&url)?;

        let body = json!({
            "markdown": {
                "content": content,
            },
            "msg_type": 2,
            "msg_seq": next_msg_seq(),
        });

        let resp = self
            .http_client()
            .post(&url)
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("QQ send message failed ({status}): {err}");
        }

        Ok(())
    }
}

#[async_trait]
impl Channel for QQChannel {
    fn name(&self) -> &str {
        "qq"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let (cleaned_text, attachments) = parse_qq_attachment_markers(&message.content);

        if attachments.is_empty() {

            return self
                .send_text_markdown(&message.recipient, &message.content)
                .await;
        }

        if !cleaned_text.is_empty() {
            self.send_text_markdown(&message.recipient, &cleaned_text)
                .await?;
        }

        for attachment in &attachments {
            if let Err(e) = self.send_attachment(&message.recipient, attachment).await {
                tracing::warn!(
                    target = attachment.target,
                    error = %e,
                    "QQ: failed to send media attachment; falling back to text"
                );

                let fallback = format!(
                    "{}: {}",
                    match attachment.kind {
                        QQMediaFileType::Image => "Image",
                        QQMediaFileType::Video => "Video",
                        QQMediaFileType::Voice => "Voice",
                        QQMediaFileType::File => "File",
                    },
                    attachment.target
                );
                self.send_text_markdown(&message.recipient, &fallback)
                    .await?;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        tracing::info!("QQ: authenticating...");
        let token = self.get_token().await?;

        tracing::info!("QQ: fetching gateway URL...");
        let gw_url = self.get_gateway_url(&token).await?;

        tracing::info!("QQ: connecting to gateway WebSocket...");
        let (ws_stream, _) = crate::services::get_services()
            .proxy_runtime()
            .ws_connect(&gw_url, "channel.qq", self.proxy_url.as_deref())
            .await?;
        let (mut write, mut read) = ws_stream.split();

        let hello = read
            .next()
            .await
            .ok_or(anyhow::anyhow!("QQ: no hello frame"))??;
        let hello_data: serde_json::Value = serde_json::from_str(&hello.to_string())?;
        let heartbeat_interval = hello_data
            .get("d")
            .and_then(|d| d.get("heartbeat_interval"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(41250);

        let stored_session = self.session_id.read().await.clone();
        let stored_seq = *self.last_sequence.read().await;

        if let (Some(sid), Some(seq)) = (&stored_session, stored_seq) {

            tracing::info!("QQ: attempting session resume (session_id={sid}, seq={seq})");
            let resume = json!({
                "op": 6,
                "d": {
                    "token": format!("QQBot {token}"),
                    "session_id": sid,
                    "seq": seq,
                }
            });
            write.send(Message::Text(resume.to_string().into())).await?;
        } else {

            let intents: u64 = (1 << 25) | (1 << 30);
            let identify = json!({
                "op": 2,
                "d": {
                    "token": format!("QQBot {token}"),
                    "intents": intents,
                    "properties": {
                        "os": "linux",
                        "browser": "sen",
                        "device": "sen",
                    }
                }
            });
            write
                .send(Message::Text(identify.to_string().into()))
                .await?;
            tracing::info!("QQ: connected and sent Identify");
        }

        let mut sequence: i64 = stored_seq.unwrap_or(-1);

        const MAX_MISSED_ACKS: u32 = 3;
        let mut missed_ack_count: u32 = 0;

        let hb_interval = heartbeat_interval;
        let grace_ms: u64 = (hb_interval / 10).min(5_000);
        let effective_interval = hb_interval.saturating_add(grace_ms);

        let (hb_tx, mut hb_rx) = tokio::sync::mpsc::channel::<()>(1);
        let _heartbeat_task =
            crate::runtime::spawn_supervised("channels.qq.heartbeat", async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_millis(effective_interval));
                loop {
                    interval.tick().await;
                    if hb_tx.send(()).await.is_err() {
                        break;
                    }
                }
            });

        enum ExitReason {
            Reconnect,
            InvalidSession,
            Close(Option<tokio_tungstenite::tungstenite::protocol::CloseFrame>),
            StreamEnded,
            HeartbeatTimeout,
            WriteFailed,
            ChannelClosed,
        }

        let exit_reason;

        'outer: loop {
            tokio::select! {
                _ = hb_rx.recv() => {

                    if missed_ack_count > 0 {
                        if missed_ack_count >= MAX_MISSED_ACKS {
                            tracing::warn!(
                                "QQ: {missed_ack_count} consecutive heartbeat ACKs missed \
                                 (interval {hb_interval}ms + {grace_ms}ms grace); \
                                 connection appears zombied"
                            );
                            exit_reason = ExitReason::HeartbeatTimeout;
                            break;
                        }
                        tracing::info!(
                            "QQ: heartbeat ACK missed ({missed_ack_count}/{MAX_MISSED_ACKS}); \
                             tolerating transient delay"
                        );
                    }
                    let d = if sequence >= 0 { json!(sequence) } else { json!(null) };
                    let hb = json!({"op": 1, "d": d});
                    if write
                        .send(Message::Text(hb.to_string().into()))
                        .await
                        .is_err()
                    {
                        exit_reason = ExitReason::WriteFailed;
                        break;
                    }
                    missed_ack_count += 1;
                }
                msg = read.next() => {
                    let msg = match msg {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Ping(payload))) => {
                            if write.send(Message::Pong(payload)).await.is_err() {
                                exit_reason = ExitReason::WriteFailed;
                                break;
                            }
                            continue;
                        }
                        Some(Ok(Message::Close(frame))) => {
                            exit_reason = ExitReason::Close(frame);
                            break;
                        }
                        None => {
                            exit_reason = ExitReason::StreamEnded;
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
                            if write
                                .send(Message::Text(hb.to_string().into()))
                                .await
                                .is_err()
                            {
                                exit_reason = ExitReason::WriteFailed;
                                break;
                            }
                            missed_ack_count += 1;
                            continue;
                        }

                        7 => {
                            tracing::warn!("QQ: received Reconnect (op 7); will resume");
                            exit_reason = ExitReason::Reconnect;
                            break;
                        }

                        9 => {
                            tracing::warn!("QQ: received Invalid Session (op 9); clearing session for fresh auth");
                            exit_reason = ExitReason::InvalidSession;
                            break;
                        }

                        11 => {
                            missed_ack_count = 0;
                            continue;
                        }
                        _ => {}
                    }

                    if op != 0 {
                        continue;
                    }

                    let event_type = event.get("t").and_then(|t| t.as_str()).unwrap_or("");
                    let d = match event.get("d") {
                        Some(d) => d,
                        None => continue,
                    };

                    if event_type == "READY" || event_type == "RESUMED" {
                        if let Some(sid) = d.get("session_id").and_then(|s| s.as_str()) {
                            *self.session_id.write().await = Some(sid.to_string());
                            tracing::info!("QQ: session established (session_id={sid}, event={event_type})");
                        }
                        continue;
                    }

                    tracing::debug!("QQ: event_type={event_type} payload={d}");

                    match event_type {
                        "C2C_MESSAGE_CREATE" => {
                            let msg_id = d.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            if self.is_duplicate(msg_id).await {
                                continue;
                            }

                            let Some(content) = self.compose_message_content(d).await else {
                                continue;
                            };

                            let author_id = d.get("author").and_then(|a| a.get("id")).and_then(|i| i.as_str()).unwrap_or("unknown");

                            let user_openid = d.get("author").and_then(|a| a.get("user_openid")).and_then(|u| u.as_str()).unwrap_or(author_id);

                            if !self.is_user_allowed(user_openid) {
                                tracing::warn!("QQ: ignoring C2C message from unauthorized user: {user_openid}");
                                continue;
                            }

                            let chat_id = format!("user:{user_openid}");

                            let channel_msg = ChannelMessage {
                                id: Uuid::new_v4().to_string(),
                                sender: user_openid.to_string(),
                                reply_target: chat_id,
                                content,
                                channel: "qq".to_string(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                thread_ts: None,
                                interruption_scope_id: None,
                    attachments: vec![],
                            };

                            if tx.send(channel_msg).await.is_err() {
                                tracing::warn!("QQ: message channel closed");
                                exit_reason = ExitReason::ChannelClosed;
                                break 'outer;
                            }
                        }
                        "GROUP_AT_MESSAGE_CREATE" => {
                            let msg_id = d.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            if self.is_duplicate(msg_id).await {
                                continue;
                            }

                            let Some(content) = self.compose_message_content(d).await else {
                                continue;
                            };

                            let author_id = d.get("author").and_then(|a| a.get("member_openid")).and_then(|m| m.as_str()).unwrap_or("unknown");

                            if !self.is_user_allowed(author_id) {
                                tracing::warn!("QQ: ignoring group message from unauthorized user: {author_id}");
                                continue;
                            }

                            let group_openid = d.get("group_openid").and_then(|g| g.as_str()).unwrap_or("unknown");
                            let chat_id = format!("group:{group_openid}");

                            let channel_msg = ChannelMessage {
                                id: Uuid::new_v4().to_string(),
                                sender: author_id.to_string(),
                                reply_target: chat_id,
                                content,
                                channel: "qq".to_string(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                thread_ts: None,
                                interruption_scope_id: None,
                    attachments: vec![],
                            };

                            if tx.send(channel_msg).await.is_err() {
                                tracing::warn!("QQ: message channel closed");
                                exit_reason = ExitReason::ChannelClosed;
                                break 'outer;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        *self.last_sequence.write().await = if sequence >= 0 { Some(sequence) } else { None };

        match exit_reason {
            ExitReason::InvalidSession => {

                *self.session_id.write().await = None;
                *self.last_sequence.write().await = None;
                anyhow::bail!(
                    "QQ WebSocket connection closed: invalid session (fresh auth required)"
                )
            }
            ExitReason::Reconnect => {

                anyhow::bail!(
                    "QQ WebSocket connection closed: server requested reconnect (resume will be attempted)"
                )
            }
            ExitReason::Close(ref frame) => {
                let (code, reason) = frame
                    .as_ref()
                    .map(|f| (f.code.to_string(), f.reason.to_string()))
                    .unwrap_or_else(|| ("unknown".into(), "none".into()));
                tracing::warn!(
                    "QQ: WebSocket closed with code={code}, reason=\"{reason}\"; \
                     resume will be attempted on reconnect"
                );
                anyhow::bail!(
                    "QQ WebSocket connection closed: close_code={code}, reason=\"{reason}\""
                )
            }
            ExitReason::StreamEnded => {
                tracing::warn!(
                    "QQ: WebSocket stream ended unexpectedly; resume will be attempted on reconnect"
                );
                anyhow::bail!("QQ WebSocket connection closed: stream ended unexpectedly")
            }
            ExitReason::HeartbeatTimeout => {
                tracing::warn!(
                    "QQ: heartbeat timeout after {MAX_MISSED_ACKS} consecutive missed ACKs; \
                     resume will be attempted on reconnect"
                );
                anyhow::bail!(
                    "QQ WebSocket connection closed: heartbeat ACK timeout \
                     ({MAX_MISSED_ACKS} consecutive missed ACKs)"
                )
            }
            ExitReason::WriteFailed => {
                tracing::warn!("QQ: WebSocket write failed; resume will be attempted on reconnect");
                anyhow::bail!("QQ WebSocket connection closed: write failed")
            }
            ExitReason::ChannelClosed => {
                anyhow::bail!("QQ WebSocket connection closed: internal message channel closed")
            }
        }
    }

    async fn health_check(&self) -> bool {
        self.fetch_access_token_with_retry().await.is_ok()
    }
}
