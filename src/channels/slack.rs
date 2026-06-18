// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use anyhow::Context;
use async_trait::async_trait;
use base64::Engine as _;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use reqwest::header::HeaderMap;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::Message as WsMessage;

#[derive(Clone)]
struct CachedSlackDisplayName {
    display_name: String,
    expires_at: Instant,
}

#[allow(clippy::struct_excessive_bools)]
pub struct SlackChannel {
    bot_token: String,
    app_token: Option<String>,
    channel_id: Option<String>,
    channel_ids: Vec<String>,
    allowed_users: Vec<String>,
    thread_replies: bool,
    mention_only: bool,
    group_reply_allowed_sender_ids: Vec<String>,
    user_display_name_cache: Mutex<HashMap<String, CachedSlackDisplayName>>,
    workspace_dir: Option<PathBuf>,

    active_assistant_thread: Mutex<HashMap<String, String>>,

    use_markdown_blocks: bool,

    proxy_url: Option<String>,

    transcription: Option<crate::config::TranscriptionConfig>,
    transcription_manager: Option<std::sync::Arc<super::pipeline::transcription::TranscriptionManager>>,

    stream_drafts: bool,

    draft_update_interval_ms: u64,

    last_draft_edit: Mutex<HashMap<String, Instant>>,

    lazy_draft_ts: tokio::sync::Mutex<HashMap<String, String>>,
}

const SLACK_HISTORY_MAX_RETRIES: u32 = 3;
const SLACK_HISTORY_DEFAULT_RETRY_AFTER_SECS: u64 = 1;
const SLACK_HISTORY_MAX_BACKOFF_SECS: u64 = 120;
const SLACK_HISTORY_MAX_JITTER_MS: u64 = 500;
const SLACK_SOCKET_MODE_INITIAL_BACKOFF_SECS: u64 = 3;
const SLACK_SOCKET_MODE_MAX_BACKOFF_SECS: u64 = 120;
const SLACK_SOCKET_MODE_MAX_JITTER_MS: u64 = 500;
const SLACK_USER_CACHE_TTL_SECS: u64 = 6 * 60 * 60;
const SLACK_ATTACHMENT_IMAGE_MAX_BYTES: usize = 5 * 1024 * 1024;
const SLACK_ATTACHMENT_IMAGE_INLINE_FALLBACK_MAX_BYTES: usize = 512 * 1024;
const SLACK_ATTACHMENT_TEXT_DOWNLOAD_MAX_BYTES: usize = 256 * 1024;
const SLACK_ATTACHMENT_TEXT_INLINE_MAX_CHARS: usize = 12_000;
const SLACK_MARKDOWN_BLOCK_MAX_CHARS: usize = 12_000;
const SLACK_BLOCK_TEXT_MAX_CHARS: usize = 3_000;
const SLACK_MAX_BLOCKS_PER_MESSAGE: usize = 50;
const SLACK_ATTACHMENT_FILENAME_MAX_CHARS: usize = 128;
const SLACK_USER_CACHE_MAX_ENTRIES: usize = 1000;
const SLACK_ATTACHMENT_SAVE_SUBDIR: &str = "slack_files";
const SLACK_ATTACHMENT_MAX_FILES_PER_MESSAGE: usize = 8;
const SLACK_PERMALINK_MAX_LINKS_PER_MESSAGE: usize = 3;
const SLACK_PERMALINK_THREAD_MAX_REPLIES: usize = 20;
const SLACK_PERMALINK_TEXT_MAX_CHARS: usize = 8_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlackPermalinkRef {
    url: String,
    channel_id: String,
    message_ts: String,
    thread_ts_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SlackPermalinkLookup {
    Message(serde_json::Value),
    AccessDenied(String),
    NotFound,
}

fn extract_slack_ts(message_id: &str) -> &str {
    message_id
        .strip_prefix("slack_")
        .and_then(|rest| {
            rest.find('.').map(|dot_pos| {
                let underscore = rest[..dot_pos].rfind('_').unwrap_or(0);
                &rest[underscore + 1..]
            })
        })
        .unwrap_or(message_id)
}

fn unicode_emoji_to_slack_name(emoji: &str) -> &str {
    match emoji {
        "\u{1F440}" => "eyes",
        "\u{2705}" => "white_check_mark",
        "\u{26A0}\u{FE0F}" | "\u{26A0}" => "warning",
        "\u{274C}" => "x",
        "\u{1F44D}" => "thumbsup",
        "\u{1F44E}" => "thumbsdown",
        "\u{2B50}" => "star",
        "\u{1F389}" => "tada",
        "\u{1F914}" => "thinking_face",
        "\u{1F525}" => "fire",
        _ => emoji.trim_matches(':'),
    }
}

const SLACK_DRAFT_UPDATE_INTERVAL_MS: u64 = 1200;

const SLACK_MESSAGE_MAX_CHARS: usize = 40_000;

const LAZY_DRAFT_PREFIX: &str = "lazy:";

const SLACK_ATTACHMENT_RENDER_CONCURRENCY: usize = 3;
const SLACK_POLL_ACTIVE_THREAD_MAX: usize = 50;
const SLACK_POLL_THREAD_EXPIRE_SECS: u64 = 24 * 60 * 60;
const SLACK_MEDIA_REDIRECT_MAX_HOPS: usize = 5;
const SLACK_ALLOWED_MEDIA_HOST_SUFFIXES: &[&str] =
    &["slack.com", "slack-edge.com", "slack-files.com"];
const SLACK_SUPPORTED_IMAGE_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/bmp",
];

impl SlackChannel {
    pub fn new(
        bot_token: String,
        app_token: Option<String>,
        channel_id: Option<String>,
        channel_ids: Vec<String>,
        allowed_users: Vec<String>,
    ) -> Self {
        Self {
            bot_token,
            app_token,
            channel_id,
            channel_ids,
            allowed_users,
            thread_replies: true,
            mention_only: false,
            group_reply_allowed_sender_ids: Vec::new(),
            user_display_name_cache: Mutex::new(HashMap::new()),
            workspace_dir: None,
            active_assistant_thread: Mutex::new(HashMap::new()),
            use_markdown_blocks: false,
            proxy_url: None,
            transcription: None,
            transcription_manager: None,
            stream_drafts: false,
            draft_update_interval_ms: SLACK_DRAFT_UPDATE_INTERVAL_MS,
            last_draft_edit: Mutex::new(HashMap::new()),
            lazy_draft_ts: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn with_group_reply_policy(
        mut self,
        mention_only: bool,
        allowed_sender_ids: Vec<String>,
    ) -> Self {
        self.mention_only = mention_only;
        self.group_reply_allowed_sender_ids =
            Self::normalize_group_reply_allowed_sender_ids(allowed_sender_ids);
        self
    }

    pub fn with_thread_replies(mut self, thread_replies: bool) -> Self {
        self.thread_replies = thread_replies;
        self
    }

    pub fn with_workspace_dir(mut self, dir: PathBuf) -> Self {
        self.workspace_dir = Some(dir);
        self
    }

    pub fn with_markdown_blocks(mut self, enabled: bool) -> Self {
        self.use_markdown_blocks = enabled;
        self
    }

    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
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

    pub fn with_streaming(mut self, enabled: bool, interval_ms: u64) -> Self {
        self.stream_drafts = enabled;
        if interval_ms > 0 {
            self.draft_update_interval_ms = interval_ms;
        }
        self
    }

    async fn delete_message(&self, channel_id: &str, ts: &str) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "channel": channel_id,
            "ts": ts,
        });

        let resp = self
            .http_client()
            .post("https://slack.com/api/chat.delete")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let resp_body: serde_json::Value = resp.json().await?;
        if resp_body.get("ok") != Some(&serde_json::Value::Bool(true)) {
            let err = resp_body
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            tracing::debug!("Slack chat.delete failed: {err}");
        }

        Ok(())
    }

    async fn resolve_draft_ts(&self, message_id: &str) -> Option<String> {
        if !message_id.starts_with(LAZY_DRAFT_PREFIX) {
            return Some(message_id.to_string());
        }
        self.lazy_draft_ts.lock().await.get(message_id).cloned()
    }

    async fn materialize_lazy_draft(
        &self,
        lazy_id: &str,
        text: &str,
    ) -> anyhow::Result<Option<String>> {

        let rest = lazy_id.strip_prefix(LAZY_DRAFT_PREFIX).unwrap_or(lazy_id);
        let (channel_id, thread_ts) = match rest.find(':') {
            Some(pos) => {
                let ts = &rest[pos + 1..];
                (&rest[..pos], if ts.is_empty() { None } else { Some(ts) })
            }
            None => (rest, None),
        };

        let mut body = serde_json::json!({
            "channel": channel_id,
            "text": text,
        });
        if text.len() <= SLACK_MARKDOWN_BLOCK_MAX_CHARS {
            body["blocks"] = serde_json::json!([{
                "type": "markdown",
                "text": text
            }]);
        }
        if let Some(ts) = thread_ts {
            body["thread_ts"] = serde_json::json!(ts);
        }

        let response = self
            .http_client()
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let resp_body: serde_json::Value = response.json().await?;
        if resp_body.get("ok") != Some(&serde_json::Value::Bool(true)) {
            let err = resp_body
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack chat.postMessage (lazy draft) failed: {err}");
        }

        let ts = resp_body
            .get("ts")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        if let Some(ref real_ts) = ts {
            self.lazy_draft_ts
                .lock()
                .await
                .insert(lazy_id.to_string(), real_ts.clone());
        }

        Ok(ts)
    }

    async fn set_assistant_status(&self, channel_id: &str, status: &str) {
        let thread_ts = {
            let map = self.active_assistant_thread.lock();
            match map.get(channel_id) {
                Some(ts) => ts.clone(),
                None => return,
            }
        };

        let body = serde_json::json!({
            "channel_id": channel_id,
            "thread_ts": thread_ts,
            "status": status,
        });

        let _ = self
            .http_client()
            .post("https://slack.com/api/assistant.threads.setStatus")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await;
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_channel_client_with_timeouts(
                "channel.slack",
                self.proxy_url.as_deref(),
                30,
                10,
            )
    }

    pub async fn post_message(&self, channel: &str, text: &str) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "channel": channel,
            "text": text,
        });

        let resp = self
            .http_client()
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&raw);
            anyhow::bail!("Slack chat.postMessage failed ({status}): {sanitized}");
        }

        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack chat.postMessage failed: {err}");
        }

        parsed
            .get("ts")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("Slack chat.postMessage response missing 'ts'"))
    }

    pub async fn update_message(&self, channel: &str, ts: &str, text: &str) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        });

        let resp = self
            .http_client()
            .post("https://slack.com/api/chat.update")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&raw);
            anyhow::bail!("Slack chat.update failed ({status}): {sanitized}");
        }

        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack chat.update failed: {err}");
        }

        Ok(())
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    fn is_group_sender_trigger_enabled(&self, user_id: &str) -> bool {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return false;
        }

        self.group_reply_allowed_sender_ids
            .iter()
            .any(|entry| entry == "*" || entry == user_id)
    }

    fn outbound_thread_ts<'a>(&self, message: &'a SendMessage) -> Option<&'a str> {
        if self.thread_replies {
            message.thread_ts.as_deref()
        } else {
            None
        }
    }

    async fn get_bot_user_id(&self) -> Option<String> {
        let resp: serde_json::Value = self
            .http_client()
            .get("https://slack.com/api/auth.test")
            .bearer_auth(&self.bot_token)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        resp.get("user_id")
            .and_then(|u| u.as_str())
            .map(String::from)
    }

    fn inbound_thread_ts(msg: &serde_json::Value, ts: &str) -> Option<String> {
        msg.get("thread_ts")
            .and_then(|t| t.as_str())
            .or(if ts.is_empty() { None } else { Some(ts) })
            .map(str::to_string)
    }

    fn inbound_thread_ts_genuine_only(msg: &serde_json::Value) -> Option<String> {
        msg.get("thread_ts")
            .and_then(|t| t.as_str())
            .map(str::to_string)
    }

    fn inbound_interruption_scope_id(msg: &serde_json::Value, ts: &str) -> Option<String> {
        msg.get("thread_ts")
            .and_then(|t| t.as_str())
            .filter(|&t| t != ts)
            .map(str::to_string)
    }

    fn normalized_channel_id(input: Option<&str>) -> Option<String> {
        input
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "*")
            .map(ToOwned::to_owned)
    }

    fn configured_channel_id(&self) -> Option<String> {
        Self::normalized_channel_id(self.channel_id.as_deref())
    }

    fn scoped_channel_ids(&self) -> Option<Vec<String>> {
        let mut seen = HashSet::new();
        let ids: Vec<String> = self
            .channel_ids
            .iter()
            .filter_map(|entry| Self::normalized_channel_id(Some(entry)))
            .filter(|id| seen.insert(id.clone()))
            .collect();
        if !ids.is_empty() {
            return Some(ids);
        }
        self.configured_channel_id().map(|id| vec![id])
    }

    fn configured_app_token(&self) -> Option<String> {
        self.app_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    fn normalize_group_reply_allowed_sender_ids(sender_ids: Vec<String>) -> Vec<String> {
        let mut normalized = sender_ids
            .into_iter()
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        normalized.sort();
        normalized.dedup();
        normalized
    }

    fn user_cache_ttl() -> Duration {
        Duration::from_secs(SLACK_USER_CACHE_TTL_SECS)
    }

    fn sanitize_display_name(name: &str) -> Option<String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn extract_user_display_name(payload: &serde_json::Value) -> Option<String> {
        let user = payload.get("user")?;
        let profile = user.get("profile");

        let candidates = [
            profile
                .and_then(|p| p.get("display_name"))
                .and_then(|v| v.as_str()),
            profile
                .and_then(|p| p.get("display_name_normalized"))
                .and_then(|v| v.as_str()),
            profile
                .and_then(|p| p.get("real_name_normalized"))
                .and_then(|v| v.as_str()),
            profile
                .and_then(|p| p.get("real_name"))
                .and_then(|v| v.as_str()),
            user.get("real_name").and_then(|v| v.as_str()),
            user.get("name").and_then(|v| v.as_str()),
        ];

        for candidate in candidates.into_iter().flatten() {
            if let Some(display_name) = Self::sanitize_display_name(candidate) {
                return Some(display_name);
            }
        }

        None
    }

    fn cached_sender_display_name(&self, user_id: &str) -> Option<String> {
        let now = Instant::now();
        let mut cache = self.user_display_name_cache.lock();

        if let Some(entry) = cache.get(user_id) {
            if now <= entry.expires_at {
                return Some(entry.display_name.clone());
            }
        }

        cache.remove(user_id);
        None
    }

    fn cache_sender_display_name(&self, user_id: &str, display_name: &str) {
        let mut cache = self.user_display_name_cache.lock();
        if cache.len() >= SLACK_USER_CACHE_MAX_ENTRIES {
            let now = Instant::now();
            cache.retain(|_, v| v.expires_at > now);
        }
        cache.insert(
            user_id.to_string(),
            CachedSlackDisplayName {
                display_name: display_name.to_string(),
                expires_at: Instant::now() + Self::user_cache_ttl(),
            },
        );
    }

    async fn fetch_sender_display_name(&self, user_id: &str) -> Option<String> {
        let resp = match self
            .http_client()
            .get("https://slack.com/api/users.info")
            .bearer_auth(&self.bot_token)
            .query(&[("user", user_id)])
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!("Slack users.info request failed for {user_id}: {err}");
                return None;
            }
        };

        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&body);
            tracing::warn!("Slack users.info failed for {user_id} ({status}): {sanitized}");
            return None;
        }

        let payload: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if payload.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = payload
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            tracing::warn!("Slack users.info returned error for {user_id}: {err}");
            return None;
        }

        Self::extract_user_display_name(&payload)
    }

    async fn resolve_sender_identity(&self, user_id: &str) -> String {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return String::new();
        }

        if let Some(display_name) = self.cached_sender_display_name(user_id) {
            return display_name;
        }

        if let Some(display_name) = self.fetch_sender_display_name(user_id).await {
            self.cache_sender_display_name(user_id, &display_name);
            return display_name;
        }

        user_id.to_string()
    }

    fn is_group_channel_id(channel_id: &str) -> bool {
        matches!(channel_id.chars().next(), Some('C' | 'G'))
    }

    fn contains_bot_mention(text: &str, bot_user_id: &str) -> bool {
        if bot_user_id.is_empty() {
            return false;
        }
        text.contains(&format!("<@{bot_user_id}>"))
    }

    fn strip_bot_mentions(text: &str, bot_user_id: &str) -> String {
        if bot_user_id.is_empty() {
            return text.trim().to_string();
        }
        text.replace(&format!("<@{bot_user_id}>"), " ")
            .trim()
            .to_string()
    }

    fn normalize_incoming_text(
        text: &str,
        require_mention: bool,
        bot_user_id: &str,
    ) -> Option<String> {
        if require_mention && !Self::contains_bot_mention(text, bot_user_id) {
            return None;
        }

        Some(Self::strip_bot_mentions(text, bot_user_id))
    }

    fn is_supported_message_subtype(subtype: Option<&str>) -> bool {
        matches!(subtype, None | Some("file_share" | "thread_broadcast"))
    }

    fn compose_incoming_content(text: String, attachment_blocks: Vec<String>) -> Option<String> {
        let mut sections = Vec::new();
        if !text.trim().is_empty() {
            sections.push(text.trim().to_string());
        }
        for block in attachment_blocks {
            if !block.trim().is_empty() {
                sections.push(block);
            }
        }

        if sections.is_empty() {
            None
        } else {
            Some(sections.join("\n\n"))
        }
    }

    async fn build_incoming_content(
        &self,
        message: &serde_json::Value,
        require_mention: bool,
        bot_user_id: &str,
    ) -> Option<String> {
        let text = message
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let normalized_text = Self::normalize_incoming_text(text, require_mention, bot_user_id)?;
        let attachment_blocks = self.render_file_attachments(message).await;
        let permalink_blocks = self.resolve_permalink_blocks(&normalized_text).await;
        let mut blocks = attachment_blocks;
        blocks.extend(permalink_blocks);
        Self::compose_incoming_content(normalized_text, blocks)
    }

    async fn resolve_permalink_blocks(&self, text: &str) -> Vec<String> {
        let permalinks = Self::extract_slack_permalinks(text);
        if permalinks.is_empty() {
            return Vec::new();
        }
        let tasks = permalinks
            .into_iter()
            .map(|permalink| async move { self.resolve_slack_permalink(&permalink).await });

        futures_util::stream::iter(tasks)
            .buffer_unordered(SLACK_ATTACHMENT_RENDER_CONCURRENCY)
            .filter_map(|block| async move { block })
            .collect()
            .await
    }

    fn extract_slack_permalinks(text: &str) -> Vec<SlackPermalinkRef> {
        let mut permalinks = Vec::new();
        let mut seen = HashSet::new();

        for token in text.split_whitespace() {
            if permalinks.len() >= SLACK_PERMALINK_MAX_LINKS_PER_MESSAGE {
                break;
            }

            let Some(url) = Self::extract_url_token(token) else {
                continue;
            };
            let Some(permalink) = Self::parse_slack_permalink(&url) else {
                continue;
            };
            if seen.insert((permalink.channel_id.clone(), permalink.message_ts.clone())) {
                permalinks.push(permalink);
            }
        }

        permalinks
    }

    fn extract_url_token(token: &str) -> Option<String> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return None;
        }

        let candidate = if trimmed.starts_with('<') && trimmed.ends_with('>') {
            trimmed
                .trim_start_matches('<')
                .trim_end_matches('>')
                .split('|')
                .next()
                .unwrap_or_default()
                .trim()
        } else {
            trimmed.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | ',' | ';'
                )
            })
        };

        if candidate.starts_with("https://") || candidate.starts_with("http://") {
            Some(candidate.to_string())
        } else {
            None
        }
    }

    fn parse_slack_permalink(raw_url: &str) -> Option<SlackPermalinkRef> {
        let url = reqwest::Url::parse(raw_url).ok()?;
        let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
        if host != "slack.com" && !host.ends_with(".slack.com") {
            return None;
        }

        let mut segments = url.path_segments()?;
        let first = segments.next()?;
        let second = segments.next()?;
        let third = segments.next()?;
        if first != "archives" || segments.next().is_some() {
            return None;
        }

        let channel_id = second.trim();
        if channel_id.is_empty() {
            return None;
        }

        let message_ts = Self::parse_slack_permalink_ts(third)?;
        let thread_ts_hint = url
            .query_pairs()
            .find(|(key, _)| key == "thread_ts")
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| Self::is_valid_slack_ts(value));

        Some(SlackPermalinkRef {
            url: raw_url.to_string(),
            channel_id: channel_id.to_string(),
            message_ts,
            thread_ts_hint,
        })
    }

    fn parse_slack_permalink_ts(segment: &str) -> Option<String> {
        let digits = segment.strip_prefix('p')?.trim();
        if digits.len() <= 6 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }

        let (secs, micros) = digits.split_at(digits.len() - 6);
        Some(format!("{secs}.{micros}"))
    }

    fn is_valid_slack_ts(ts: &str) -> bool {
        let Some((secs, micros)) = ts.split_once('.') else {
            return false;
        };
        !secs.is_empty()
            && micros.len() == 6
            && secs.chars().all(|ch| ch.is_ascii_digit())
            && micros.chars().all(|ch| ch.is_ascii_digit())
    }

    async fn resolve_slack_permalink(&self, permalink: &SlackPermalinkRef) -> Option<String> {
        let message_lookup = self
            .fetch_permalink_message(&permalink.channel_id, &permalink.message_ts)
            .await;
        let message = match message_lookup {
            SlackPermalinkLookup::Message(message) => message,
            SlackPermalinkLookup::AccessDenied(reason) => {
                return Some(Self::format_permalink_access_denied(permalink, &reason));
            }
            SlackPermalinkLookup::NotFound => {
                let thread_ts = permalink.thread_ts_hint.as_deref()?;
                let replies = self
                    .fetch_thread_messages_with_retry(&permalink.channel_id, thread_ts)
                    .await?;
                let target = replies.into_iter().find(|reply| {
                    reply.get("ts").and_then(|value| value.as_str())
                        == Some(permalink.message_ts.as_str())
                });
                let target = target?;
                return self
                    .format_permalink_context(permalink, target, Some(thread_ts))
                    .await;
            }
        };

        let thread_ts = message
            .get("thread_ts")
            .and_then(|value| value.as_str())
            .filter(|thread_ts| Self::is_valid_slack_ts(thread_ts))
            .map(str::to_string);

        let formatted = self
            .format_permalink_context(permalink, message, thread_ts.as_deref())
            .await;
        formatted
    }

    async fn fetch_permalink_message(
        &self,
        channel_id: &str,
        message_ts: &str,
    ) -> SlackPermalinkLookup {
        let resp = match self
            .http_client()
            .get("https://slack.com/api/conversations.history")
            .bearer_auth(&self.bot_token)
            .query(&[
                ("channel", channel_id),
                ("oldest", message_ts),
                ("latest", message_ts),
                ("inclusive", "true"),
                ("limit", "1"),
            ])
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!(
                    "Slack permalink resolver: conversations.history request failed for channel={} ts={}: {}",
                    channel_id,
                    message_ts,
                    err
                );
                return SlackPermalinkLookup::NotFound;
            }
        };

        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&body);
            tracing::warn!(
                "Slack permalink resolver: conversations.history failed for channel={} ts={} ({}): {}",
                channel_id,
                message_ts,
                status,
                sanitized
            );
            return SlackPermalinkLookup::NotFound;
        }

        let payload: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if payload.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = payload
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            return match err {
                "not_in_channel" => SlackPermalinkLookup::AccessDenied(
                    "The Slack bot is not in that channel. Invite the app to the channel and try again."
                        .to_string(),
                ),
                "missing_scope" => SlackPermalinkLookup::AccessDenied(
                    "The Slack app is missing the scope needed to read that channel."
                        .to_string(),
                ),
                _ => {
                    tracing::warn!(
                        "Slack permalink resolver: conversations.history returned error for channel={} ts={}: {}",
                        channel_id, message_ts, err
                    );
                    SlackPermalinkLookup::NotFound
                }
            };
        }

        let messages = payload
            .get("messages")
            .and_then(|messages| messages.as_array())
            .cloned()
            .unwrap_or_default();
        messages
            .first()
            .cloned()
            .map(SlackPermalinkLookup::Message)
            .unwrap_or(SlackPermalinkLookup::NotFound)
    }

    fn format_permalink_access_denied(permalink: &SlackPermalinkRef, reason: &str) -> String {
        format!(
            "[Slack Link Access]\nURL: {}\nStatus: {}",
            permalink.url, reason
        )
    }

    async fn fetch_thread_messages_with_retry(
        &self,
        channel_id: &str,
        thread_ts: &str,
    ) -> Option<Vec<serde_json::Value>> {
        let payload = self
            .fetch_thread_replies_with_retry(channel_id, thread_ts, "0")
            .await?;
        let messages = payload
            .get("messages")
            .and_then(|messages| messages.as_array())
            .cloned()
            .unwrap_or_default();
        Some(messages)
    }

    async fn format_permalink_context(
        &self,
        permalink: &SlackPermalinkRef,
        message: serde_json::Value,
        thread_ts: Option<&str>,
    ) -> Option<String> {
        let mut lines = vec![
            "[Slack Link Context]".to_string(),
            format!("URL: {}", permalink.url),
        ];

        if let Some(thread_ts) = thread_ts {
            let replies = self
                .fetch_thread_messages_with_retry(&permalink.channel_id, thread_ts)
                .await
                .unwrap_or_else(|| vec![message.clone()]);
            let rendered = self
                .render_permalink_thread_messages(&replies, &permalink.message_ts)
                .await;
            if rendered.is_empty() {
                return None;
            }
            lines.push("Thread:".to_string());
            lines.extend(rendered);
        } else {
            let rendered = self.render_permalink_message_line(&message, true).await?;
            lines.push("Message:".to_string());
            lines.push(rendered);
        }

        Self::truncate_text(&lines.join("\n"), SLACK_PERMALINK_TEXT_MAX_CHARS)
    }

    async fn render_permalink_thread_messages(
        &self,
        messages: &[serde_json::Value],
        target_ts: &str,
    ) -> Vec<String> {
        let mut rendered = Vec::new();
        let total = messages.len();
        let start = total.saturating_sub(SLACK_PERMALINK_THREAD_MAX_REPLIES);

        if start > 0 {
            rendered.push(format!("… {} earlier thread messages omitted …", start));
        }

        for message in &messages[start..] {
            if let Some(line) = self
                .render_permalink_message_line(
                    message,
                    message.get("ts").and_then(|value| value.as_str()) == Some(target_ts),
                )
                .await
            {
                rendered.push(line);
            }
        }

        rendered
    }

    async fn render_permalink_message_line(
        &self,
        message: &serde_json::Value,
        highlight: bool,
    ) -> Option<String> {
        let user_id = message
            .get("user")
            .or_else(|| message.get("bot_id"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let sender = if user_id.is_empty() {
            "unknown".to_string()
        } else {
            self.resolve_sender_identity(user_id).await
        };

        let text = message
            .get("text")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("[no text]");
        let attachment_blocks = self.render_file_attachments(message).await;
        let content = Self::compose_incoming_content(text.to_string(), attachment_blocks)
            .unwrap_or_else(|| text.to_string())
            .replace('\n', " ");
        let prefix = if highlight { ">" } else { "-" };
        Some(format!("{prefix} {sender}: {content}"))
    }

    async fn render_file_attachments(&self, message: &serde_json::Value) -> Vec<String> {
        let Some(files) = message.get("files").and_then(|value| value.as_array()) else {
            return Vec::new();
        };

        if files.len() > SLACK_ATTACHMENT_MAX_FILES_PER_MESSAGE {
            tracing::warn!(
                "Slack message has {} files; processing first {} only",
                files.len(),
                SLACK_ATTACHMENT_MAX_FILES_PER_MESSAGE
            );
        }

        let limited_files = files
            .iter()
            .take(SLACK_ATTACHMENT_MAX_FILES_PER_MESSAGE)
            .cloned()
            .collect::<Vec<_>>();

        let tasks =
            limited_files
                .into_iter()
                .enumerate()
                .map(|(idx, raw_file)| async move {
                    (idx, self.render_file_attachment(&raw_file).await)
                });

        let mut rendered = futures_util::stream::iter(tasks)
            .buffer_unordered(SLACK_ATTACHMENT_RENDER_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        rendered.sort_by_key(|(idx, _)| *idx);
        rendered
            .into_iter()
            .filter_map(|(_, block)| block)
            .collect()
    }

    async fn render_file_attachment(&self, raw_file: &serde_json::Value) -> Option<String> {
        let file = self
            .hydrate_file_object(raw_file)
            .await
            .unwrap_or_else(|| raw_file.clone());

        if Self::is_audio_file(&file) {
            if let Some(transcribed) = self.try_transcribe_audio_file(&file).await {
                return Some(transcribed);
            }
        }
        if Self::is_image_file(&file) {
            if let Some(marker) = self.fetch_image_marker(&file).await {
                return Some(marker);
            }
        }

        let mut snippet = Self::file_text_preview(&file);
        if snippet.is_none() && Self::is_probably_text_file(&file) {
            snippet = self.download_text_snippet(&file).await;
        }

        if let Some(text) = snippet {
            if !text.trim().is_empty() {
                return Some(Self::format_snippet_attachment(&file, &text));
            }
        }

        Some(Self::format_attachment_summary(&file))
    }

    async fn hydrate_file_object(&self, file: &serde_json::Value) -> Option<serde_json::Value> {
        let file_id = Self::slack_file_id(file)?;
        let file_access = file
            .get("file_access")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let mode = Self::slack_file_mode(file).unwrap_or_default();

        let requires_lookup = file_access.eq_ignore_ascii_case("check_file_info")
            || Self::slack_file_download_url(file).is_none()
            || (Self::is_probably_text_file(file) && Self::file_text_preview(file).is_none())
            || (mode == "snippet" && file.get("preview").is_none());
        if !requires_lookup {
            return Some(file.clone());
        }

        self.fetch_file_info(file_id)
            .await
            .or_else(|| Some(file.clone()))
    }

    async fn fetch_file_info(&self, file_id: &str) -> Option<serde_json::Value> {
        let resp = match self
            .http_client()
            .get("https://slack.com/api/files.info")
            .bearer_auth(&self.bot_token)
            .query(&[("file", file_id)])
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!("Slack files.info request failed for {file_id}: {err}");
                return None;
            }
        };

        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&body);
            tracing::warn!("Slack files.info failed for {file_id} ({status}): {sanitized}");
            return None;
        }

        let payload: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if payload.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = payload
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            tracing::warn!("Slack files.info returned error for {file_id}: {err}");
            return None;
        }

        payload.get("file").cloned()
    }

    fn slack_file_id(file: &serde_json::Value) -> Option<&str> {
        file.get("id").and_then(|value| value.as_str())
    }

    fn slack_file_name(file: &serde_json::Value) -> String {
        file.get("title")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| file.get("name").and_then(|value| value.as_str()))
            .unwrap_or("attachment")
            .trim()
            .to_string()
    }

    fn slack_file_mode(file: &serde_json::Value) -> Option<String> {
        file.get("mode")
            .and_then(|value| value.as_str())
            .map(|value| value.to_ascii_lowercase())
    }

    fn slack_file_mime(file: &serde_json::Value) -> Option<String> {
        file.get("mimetype")
            .and_then(|value| value.as_str())
            .map(|value| value.to_ascii_lowercase())
    }

    fn slack_file_download_url(file: &serde_json::Value) -> Option<&str> {
        file.get("url_private_download")
            .and_then(|value| value.as_str())
            .or_else(|| file.get("url_private").and_then(|value| value.as_str()))
    }

    fn slack_image_candidate_urls(file: &serde_json::Value) -> Vec<String> {
        let mut urls = Vec::new();
        let mut seen = HashSet::new();
        for key in [
            "thumb_1024",
            "thumb_960",
            "thumb_800",
            "thumb_720",
            "thumb_480",
            "thumb_360",
            "thumb_160",
            "url_private_download",
            "url_private",
        ] {
            if let Some(url) = file.get(key).and_then(|value| value.as_str()) {
                let trimmed = url.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if seen.insert(trimmed.to_string()) {
                    urls.push(trimmed.to_string());
                }
            }
        }
        urls
    }

    fn is_allowed_slack_media_hostname(host: &str) -> bool {
        let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if normalized.is_empty() {
            return false;
        }

        SLACK_ALLOWED_MEDIA_HOST_SUFFIXES
            .iter()
            .any(|suffix| normalized == *suffix || normalized.ends_with(&format!(".{suffix}")))
    }

    fn redact_slack_url(url: &reqwest::Url) -> String {
        let host = url.host_str().unwrap_or("unknown-host");
        let tail = url
            .path_segments()
            .and_then(|mut segments| {
                segments
                    .rfind(|segment| !segment.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "root".to_string());
        format!("{host}/.../{tail}")
    }

    fn redact_raw_slack_url(raw_url: &str) -> String {
        reqwest::Url::parse(raw_url)
            .map(|parsed| Self::redact_slack_url(&parsed))
            .unwrap_or_else(|_| "<invalid-url>".to_string())
    }

    fn redact_redirect_location(location: &str) -> String {
        match reqwest::Url::parse(location) {
            Ok(url) => Self::redact_slack_url(&url),
            Err(_) => {
                let tail = location
                    .split(['?', '#'])
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .filter(|segment| !segment.is_empty())
                    .unwrap_or("relative");
                format!("relative/.../{tail}")
            }
        }
    }

    fn validate_slack_private_file_url(raw_url: &str) -> Option<reqwest::Url> {
        let parsed = match reqwest::Url::parse(raw_url) {
            Ok(url) => url,
            Err(err) => {
                let redacted_raw = Self::redact_raw_slack_url(raw_url);
                tracing::warn!("Slack file URL parse failed for {redacted_raw}: {err}");
                return None;
            }
        };
        let redacted = Self::redact_slack_url(&parsed);

        if parsed.scheme() != "https" {
            tracing::warn!(
                "Slack file URL rejected due to non-HTTPS scheme for {}: {}",
                redacted,
                parsed.scheme()
            );
            return None;
        }

        let Some(host) = parsed.host_str() else {
            tracing::warn!("Slack file URL rejected due to missing host: {redacted}");
            return None;
        };
        if !Self::is_allowed_slack_media_hostname(host) {
            tracing::warn!("Slack file URL rejected due to non-Slack host: {redacted}");
            return None;
        }

        Some(parsed)
    }

    fn resolve_https_redirect_target(base: &reqwest::Url, location: &str) -> Option<reqwest::Url> {
        let redacted_base = Self::redact_slack_url(base);
        let redacted_location = Self::redact_redirect_location(location);
        let target = match base.join(location) {
            Ok(url) => url,
            Err(err) => {
                tracing::warn!(
                    "Slack file redirect URL parse failed for base {} and location {}: {}",
                    redacted_base,
                    redacted_location,
                    err
                );
                return None;
            }
        };
        let redacted_target = Self::redact_slack_url(&target);
        if target.scheme() != "https" {
            tracing::warn!(
                "Slack file redirect rejected due to non-HTTPS scheme for {}",
                redacted_target
            );
            return None;
        }
        let Some(host) = target.host_str() else {
            tracing::warn!(
                "Slack file redirect rejected due to missing host for {}",
                redacted_target
            );
            return None;
        };
        if !Self::is_allowed_slack_media_hostname(host) {
            tracing::warn!(
                "Slack file redirect rejected due to non-Slack host for {}",
                redacted_target
            );
            return None;
        }
        Some(target)
    }

    fn slack_media_http_client_no_redirect(&self) -> anyhow::Result<reqwest::Client> {
        let builder = crate::services::require_services()
            .proxy_runtime()
            .apply_channel_to_builder(
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(Duration::from_secs(30))
                    .connect_timeout(Duration::from_secs(10)),
                "channel.slack",
                self.proxy_url.as_deref(),
            );
        builder
            .build()
            .context("failed to build Slack media no-redirect HTTP client")
    }

    async fn fetch_slack_private_file(&self, raw_url: &str) -> Option<reqwest::Response> {
        let parsed = Self::validate_slack_private_file_url(raw_url)?;
        let redacted_parsed = Self::redact_slack_url(&parsed);
        let client = match self.slack_media_http_client_no_redirect() {
            Ok(client) => client,
            Err(err) => {
                tracing::warn!("Slack file fetch failed for {}: {}", redacted_parsed, err);
                return None;
            }
        };
        let mut current_url = parsed;

        for redirect_hop in 0..=SLACK_MEDIA_REDIRECT_MAX_HOPS {
            let redacted_current = Self::redact_slack_url(&current_url);
            let mut req = client.get(current_url.clone());
            if redirect_hop == 0 {
                req = req.bearer_auth(&self.bot_token);
            }
            let response = match req.send().await {
                Ok(response) => response,
                Err(err) => {
                    tracing::warn!("Slack file fetch failed for {}: {}", redacted_current, err);
                    return None;
                }
            };

            if !response.status().is_redirection() {
                return Some(response);
            }

            if redirect_hop == SLACK_MEDIA_REDIRECT_MAX_HOPS {
                tracing::warn!(
                    "Slack file redirect limit exceeded for {} after {} hops",
                    redacted_current,
                    SLACK_MEDIA_REDIRECT_MAX_HOPS
                );
                return Some(response);
            }

            let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                return Some(response);
            };
            let Ok(location) = location.to_str() else {
                tracing::warn!(
                    "Slack file redirect location header is not valid UTF-8 for {}",
                    redacted_current
                );
                return Some(response);
            };
            let Some(next_url) = Self::resolve_https_redirect_target(&current_url, location) else {
                return Some(response);
            };
            current_url = next_url;
        }

        None
    }

    async fn fetch_image_marker(&self, file: &serde_json::Value) -> Option<String> {
        let file_name = Self::slack_file_name(file);
        let image_urls = Self::slack_image_candidate_urls(file);
        if image_urls.is_empty() {
            tracing::warn!(
                "Slack file attachment is image-like but has no downloadable URL: {}",
                file_name
            );
            return None;
        }

        for url in image_urls {
            if let Some(marker) = self.download_private_image_as_marker(&url, file).await {
                return Some(marker);
            }
        }

        tracing::warn!("Slack image attachment download failed for {file_name}");
        None
    }

    async fn download_private_image_as_marker(
        &self,
        url: &str,
        file: &serde_json::Value,
    ) -> Option<String> {
        let redacted_url = Self::redact_raw_slack_url(url);
        let resp = self.fetch_slack_private_file(url).await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            let sanitized = crate::providers::sanitize_api_error(&body);
            tracing::warn!(
                "Slack image fetch failed for {} ({status}): {sanitized}",
                redacted_url
            );
            return None;
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if let Some(content_length) = resp.content_length() {
            let content_length = usize::try_from(content_length).unwrap_or(usize::MAX);
            if content_length > SLACK_ATTACHMENT_IMAGE_MAX_BYTES {
                tracing::warn!(
                    "Slack image fetch skipped for {}: content-length {} exceeds {} bytes",
                    redacted_url,
                    content_length,
                    SLACK_ATTACHMENT_IMAGE_MAX_BYTES
                );
                return None;
            }
        }

        let bytes = match resp.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!("Slack image body read failed for {}: {err}", redacted_url);
                return None;
            }
        };
        if bytes.is_empty() {
            tracing::warn!("Slack image body is empty for {}", redacted_url);
            return None;
        }
        if bytes.len() > SLACK_ATTACHMENT_IMAGE_MAX_BYTES {
            tracing::warn!(
                "Slack image body too large for {}: {} bytes exceeds {} bytes",
                redacted_url,
                bytes.len(),
                SLACK_ATTACHMENT_IMAGE_MAX_BYTES
            );
            return None;
        }

        let Some(mime) =
            Self::detect_image_mime(content_type.as_deref(), file, bytes.as_ref(), url)
        else {
            tracing::warn!("Slack image MIME detection failed for {}", redacted_url);
            return None;
        };
        if !Self::is_supported_image_mime(&mime) {
            tracing::warn!(
                "Slack image MIME not supported for {}: {mime}",
                redacted_url
            );
            return None;
        }

        let file_name = Self::slack_file_name(file);
        if let Some(saved_path) = self
            .persist_image_attachment(file, &file_name, &mime, bytes.as_ref())
            .await
        {
            return Some(format!("[IMAGE:{}]", saved_path.display()));
        }

        if bytes.len() > SLACK_ATTACHMENT_IMAGE_INLINE_FALLBACK_MAX_BYTES {
            tracing::warn!(
                "Slack image inline fallback skipped for {}: {} bytes exceeds {} bytes",
                redacted_url,
                bytes.len(),
                SLACK_ATTACHMENT_IMAGE_INLINE_FALLBACK_MAX_BYTES
            );
            return None;
        }

        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Some(format!("[IMAGE:data:{mime};base64,{encoded}]"))
    }

    fn detect_image_mime(
        content_type_header: Option<&str>,
        file: &serde_json::Value,
        bytes: &[u8],
        source_url: &str,
    ) -> Option<String> {
        let redacted_source = Self::redact_raw_slack_url(source_url);
        if let Some(magic_mime) = Self::mime_from_magic(bytes) {
            return Some(magic_mime.to_string());
        }

        if let Some(header_mime) = content_type_header
            .and_then(Self::normalized_content_type)
            .filter(|mime| mime.starts_with("image/"))
        {
            tracing::warn!(
                "Slack image MIME mismatch for {}: HTTP header claims {}, but bytes do not match a supported image signature",
                redacted_source,
                header_mime
            );
        }

        if let Some(file_mime) =
            Self::slack_file_mime(file).filter(|mime| mime.starts_with("image/"))
        {
            tracing::warn!(
                "Slack image MIME mismatch for {}: file metadata claims {}, but bytes do not match a supported image signature",
                redacted_source,
                file_mime
            );
        }

        if let Some(ext) = Self::file_extension(source_url)
            .or_else(|| Self::file_extension(&Self::slack_file_name(file)))
        {
            if let Some(mime) = Self::mime_from_extension(&ext) {
                tracing::warn!(
                    "Slack image MIME mismatch for {}: filename extension implies {}, but bytes do not match a supported image signature",
                    redacted_source,
                    mime
                );
            }
        }

        None
    }

    fn normalized_content_type(content_type: &str) -> Option<String> {
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if mime.is_empty() { None } else { Some(mime) }
    }

    fn is_supported_image_mime(mime: &str) -> bool {
        SLACK_SUPPORTED_IMAGE_MIME_TYPES.contains(&mime)
    }

    fn mime_from_extension(ext: &str) -> Option<&'static str> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            "bmp" => Some("image/bmp"),
            _ => None,
        }
    }

    fn mime_from_magic(bytes: &[u8]) -> Option<&'static str> {
        if bytes.len() >= 8
            && bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'])
        {
            return Some("image/png");
        }
        if bytes.len() >= 3 && bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return Some("image/jpeg");
        }
        if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
            return Some("image/gif");
        }
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return Some("image/webp");
        }
        if bytes.len() >= 2 && bytes.starts_with(b"BM") {
            return Some("image/bmp");
        }
        None
    }

    async fn persist_image_attachment(
        &self,
        file: &serde_json::Value,
        file_name: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Option<PathBuf> {
        let workspace = self.workspace_dir.as_ref()?;
        let safe_name = Self::sanitize_attachment_filename(file_name)
            .unwrap_or_else(|| "attachment".to_string());
        let ext = Self::image_extension_for_mime(mime).unwrap_or("png");
        let safe_name = Self::ensure_file_extension(&safe_name, ext);
        let file_id = Self::slack_file_id(file)
            .map(Self::sanitize_file_id)
            .unwrap_or_else(|| "file".to_string());
        let generated_name = format!(
            "slack_{}_{}_{}",
            Utc::now().timestamp_millis(),
            file_id,
            safe_name
        );

        let output_path = match Self::resolve_workspace_attachment_output_path(
            workspace,
            &generated_name,
        )
        .await
        {
            Ok(path) => path,
            Err(err) => {
                tracing::warn!(
                    "Slack image attachment path resolution failed for {}: {err}",
                    file_name
                );
                return None;
            }
        };

        let Some(parent_dir) = output_path.parent() else {
            tracing::warn!(
                "Slack image attachment write failed for {}: missing parent directory",
                output_path.display()
            );
            return None;
        };

        let file_tail = output_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment");
        let temp_name = format!(
            ".{file_tail}.{}.part",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let temp_path = parent_dir.join(temp_name);

        let mut temp_file = match tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .await
        {
            Ok(file) => file,
            Err(err) => {
                tracing::warn!(
                    "Slack image attachment temp open failed for {}: {err}",
                    temp_path.display()
                );
                return None;
            }
        };

        if let Err(err) = temp_file.write_all(bytes).await {
            tracing::warn!(
                "Slack image attachment temp write failed for {}: {err}",
                temp_path.display()
            );
            let _ = tokio::fs::remove_file(&temp_path).await;
            return None;
        }
        if let Err(err) = temp_file.sync_all().await {
            tracing::warn!(
                "Slack image attachment temp sync failed for {}: {err}",
                temp_path.display()
            );
            let _ = tokio::fs::remove_file(&temp_path).await;
            return None;
        }
        drop(temp_file);

        match tokio::fs::symlink_metadata(&output_path).await {
            Ok(meta) if meta.file_type().is_symlink() => {
                tracing::warn!(
                    "Slack image attachment refused: output path is a symlink: {}",
                    output_path.display()
                );
                let _ = tokio::fs::remove_file(&temp_path).await;
                return None;
            }
            _ => {}
        }

        if let Err(err) = tokio::fs::rename(&temp_path, &output_path).await {
            tracing::warn!(
                "Slack image attachment finalize failed for {}: {err}",
                output_path.display()
            );
            let _ = tokio::fs::remove_file(&temp_path).await;
            return None;
        }

        Some(output_path)
    }

    async fn resolve_workspace_attachment_output_path(
        workspace: &Path,
        file_name: &str,
    ) -> anyhow::Result<PathBuf> {
        let safe_name = Self::sanitize_attachment_filename(file_name)
            .ok_or_else(|| anyhow::anyhow!("invalid attachment filename: {file_name}"))?;

        tokio::fs::create_dir_all(workspace).await?;
        let workspace_root = tokio::fs::canonicalize(workspace)
            .await
            .unwrap_or_else(|_| workspace.to_path_buf());

        let save_dir = workspace.join(SLACK_ATTACHMENT_SAVE_SUBDIR);
        tokio::fs::create_dir_all(&save_dir).await?;
        let resolved_save_dir = tokio::fs::canonicalize(&save_dir).await.with_context(|| {
            format!(
                "failed to resolve Slack attachment save directory: {}",
                save_dir.display()
            )
        })?;

        if !resolved_save_dir.starts_with(&workspace_root) {
            anyhow::bail!(
                "Slack attachment save directory escapes workspace: {}",
                resolved_save_dir.display()
            );
        }

        Ok(resolved_save_dir.join(safe_name))
    }

    fn sanitize_attachment_filename(file_name: &str) -> Option<String> {
        let basename = Path::new(file_name).file_name()?.to_str()?.trim();
        if basename.is_empty() || basename == "." || basename == ".." {
            return None;
        }

        let sanitized: String = basename
            .replace(['/', '\\'], "_")
            .chars()
            .take(SLACK_ATTACHMENT_FILENAME_MAX_CHARS)
            .collect();
        if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
            None
        } else {
            Some(sanitized)
        }
    }

    fn sanitize_file_id(file_id: &str) -> String {
        let cleaned: String = file_id
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            .take(64)
            .collect();
        if cleaned.is_empty() {
            "file".to_string()
        } else {
            cleaned
        }
    }

    fn ensure_file_extension(file_name: &str, extension: &str) -> String {
        if Path::new(file_name).extension().is_some() {
            file_name.to_string()
        } else {
            format!("{file_name}.{extension}")
        }
    }

    fn image_extension_for_mime(mime: &str) -> Option<&'static str> {
        match mime {
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpg"),
            "image/webp" => Some("webp"),
            "image/gif" => Some("gif"),
            "image/bmp" => Some("bmp"),
            _ => None,
        }
    }

    fn file_extension(value: &str) -> Option<String> {
        let before_query = value.split('?').next().unwrap_or(value);
        before_query
            .rsplit('/')
            .next()
            .unwrap_or(before_query)
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
    }

    fn file_text_preview(file: &serde_json::Value) -> Option<String> {
        let preview = file
            .get("preview")
            .and_then(|value| value.as_str())
            .or_else(|| {
                file.get("preview_highlight")
                    .and_then(|value| value.as_str())
            })
            .or_else(|| {
                file.get("initial_comment")
                    .and_then(|comment| comment.get("comment"))
                    .and_then(|value| value.as_str())
            })?;
        Self::truncate_text(preview, SLACK_ATTACHMENT_TEXT_INLINE_MAX_CHARS)
    }

    fn truncate_text(value: &str, max_chars: usize) -> Option<String> {
        let mut out = String::new();
        let mut count = 0usize;
        for ch in value.chars() {
            if count >= max_chars {
                break;
            }
            out.push(ch);
            count += 1;
        }
        let was_truncated = count >= max_chars && value.chars().nth(max_chars).is_some();
        let mut out = out.trim().to_string();
        if out.is_empty() {
            return None;
        }
        if was_truncated {
            out.push_str("\n…[truncated]");
        }
        Some(out)
    }

    fn is_probably_text_file(file: &serde_json::Value) -> bool {
        if matches!(
            Self::slack_file_mode(file).as_deref(),
            Some("snippet" | "post")
        ) {
            return true;
        }

        if Self::slack_file_mime(file)
            .as_deref()
            .is_some_and(|mime| mime.starts_with("text/"))
        {
            return true;
        }

        if file
            .get("filetype")
            .and_then(|value| value.as_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
            .is_some_and(Self::is_text_filetype)
        {
            return true;
        }

        Self::file_extension(&Self::slack_file_name(file))
            .as_deref()
            .is_some_and(Self::is_text_filetype)
    }

    fn is_text_filetype(filetype: &str) -> bool {
        matches!(
            filetype,
            "txt"
                | "text"
                | "md"
                | "markdown"
                | "csv"
                | "tsv"
                | "json"
                | "yaml"
                | "yml"
                | "toml"
                | "xml"
                | "html"
                | "css"
                | "js"
                | "ts"
                | "jsx"
                | "tsx"
                | "py"
                | "rs"
                | "go"
                | "java"
                | "kt"
                | "c"
                | "cc"
                | "cpp"
                | "h"
                | "hpp"
                | "cs"
                | "php"
                | "rb"
                | "swift"
                | "sql"
                | "log"
                | "ini"
                | "conf"
                | "cfg"
                | "env"
                | "sh"
                | "bash"
                | "zsh"
        )
    }

    fn is_image_file(file: &serde_json::Value) -> bool {
        if Self::slack_file_mime(file)
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            return true;
        }

        if file
            .get("filetype")
            .and_then(|value| value.as_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
            .is_some_and(|filetype| Self::mime_from_extension(filetype).is_some())
        {
            return true;
        }

        Self::file_extension(&Self::slack_file_name(file))
            .as_deref()
            .is_some_and(|ext| Self::mime_from_extension(ext).is_some())
    }

    const AUDIO_EXTENSIONS: &[&str] = &[
        "flac", "mp3", "mpeg", "mpga", "mp4", "m4a", "ogg", "oga", "opus", "wav", "webm",
    ];

    fn is_audio_file(file: &serde_json::Value) -> bool {

        if let Some(subtype) = file.get("subtype").and_then(|v| v.as_str()) {
            if subtype == "slack_audio" {
                return true;
            }
        }

        if Self::slack_file_mime(file)
            .as_deref()
            .is_some_and(|mime| mime.starts_with("audio/"))
        {
            return true;
        }

        if let Some(ft) = file
            .get("filetype")
            .and_then(|v| v.as_str())
            .map(|v| v.to_ascii_lowercase())
        {
            if Self::AUDIO_EXTENSIONS.contains(&ft.as_str()) {
                return true;
            }
        }

        Self::file_extension(&Self::slack_file_name(file))
            .as_deref()
            .is_some_and(|ext| Self::AUDIO_EXTENSIONS.contains(&ext))
    }

    async fn try_transcribe_audio_file(&self, file: &serde_json::Value) -> Option<String> {
        let manager = self.transcription_manager.as_deref()?;

        let url = Self::slack_file_download_url(file)?;
        let file_name = Self::slack_file_name(file);
        let redacted_url = Self::redact_raw_slack_url(url);

        let resp = self.fetch_slack_private_file(url).await?;
        let status = resp.status();
        if !status.is_success() {
            tracing::warn!(
                "Slack voice file download failed for {} ({status})",
                redacted_url
            );
            return None;
        }

        let audio_data = match resp.bytes().await {
            Ok(bytes) => bytes.to_vec(),
            Err(e) => {
                tracing::warn!("Slack voice file read failed for {}: {e}", redacted_url);
                return None;
            }
        };

        let transcription_filename = if Self::file_extension(&file_name).is_some() {
            file_name.clone()
        } else {

            let mime_ext = Self::slack_file_mime(file)
                .and_then(|mime| mime.rsplit('/').next().map(|s| s.to_string()))
                .unwrap_or_else(|| "ogg".to_string());
            format!("voice.{mime_ext}")
        };

        match manager
            .transcribe(&audio_data, &transcription_filename)
            .await
        {
            Ok(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    tracing::info!("Slack voice transcription returned empty text, skipping");
                    None
                } else {
                    tracing::info!(
                        "Slack: transcribed voice file {} ({} chars)",
                        file_name,
                        trimmed.len()
                    );
                    Some(format!("[Voice] {trimmed}"))
                }
            }
            Err(e) => {
                tracing::warn!("Slack voice transcription failed for {}: {e}", file_name);
                Some(Self::format_attachment_summary(file))
            }
        }
    }

    async fn download_text_snippet(&self, file: &serde_json::Value) -> Option<String> {
        let url = Self::slack_file_download_url(file)?;
        let redacted_url = Self::redact_raw_slack_url(url);
        let resp = self.fetch_slack_private_file(url).await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            let sanitized = crate::providers::sanitize_api_error(&body);
            tracing::warn!(
                "Slack snippet fetch failed for {} ({status}): {sanitized}",
                redacted_url
            );
            return None;
        }

        if let Some(content_length) = resp.content_length() {
            let content_length = usize::try_from(content_length).unwrap_or(usize::MAX);
            if content_length > SLACK_ATTACHMENT_TEXT_DOWNLOAD_MAX_BYTES {
                tracing::warn!(
                    "Slack snippet download skipped for {}: content-length {} exceeds {} bytes",
                    redacted_url,
                    content_length,
                    SLACK_ATTACHMENT_TEXT_DOWNLOAD_MAX_BYTES
                );
                return None;
            }
        }

        let bytes = match resp.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!("Slack snippet body read failed for {}: {err}", redacted_url);
                return None;
            }
        };
        if bytes.is_empty() {
            return None;
        }
        if bytes.len() > SLACK_ATTACHMENT_TEXT_DOWNLOAD_MAX_BYTES {
            tracing::warn!(
                "Slack snippet body too large for {}: {} bytes exceeds {} bytes",
                redacted_url,
                bytes.len(),
                SLACK_ATTACHMENT_TEXT_DOWNLOAD_MAX_BYTES
            );
            return None;
        }
        if bytes.contains(&0) {
            tracing::warn!("Slack snippet body appears binary for {}", redacted_url);
            return None;
        }

        let text = String::from_utf8_lossy(&bytes);
        Self::truncate_text(&text, SLACK_ATTACHMENT_TEXT_INLINE_MAX_CHARS)
    }

    fn format_snippet_attachment(file: &serde_json::Value, snippet: &str) -> String {
        let file_name = Self::slack_file_name(file);
        let language = file
            .get("filetype")
            .and_then(|value| value.as_str())
            .map(Self::sanitize_code_fence_language)
            .unwrap_or_else(|| "text".to_string());

        let fence = if snippet.contains("```") {
            "````"
        } else {
            "```"
        };
        format!("[SNIPPET:{file_name}]\n{fence}{language}\n{snippet}\n{fence}")
    }

    fn sanitize_code_fence_language(input: &str) -> String {
        let normalized = input
            .trim()
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+'))
            .collect::<String>();
        if normalized.is_empty() {
            "text".to_string()
        } else {
            normalized
        }
    }

    fn format_attachment_summary(file: &serde_json::Value) -> String {
        let file_name = Self::slack_file_name(file);
        let mime = Self::slack_file_mime(file).unwrap_or_else(|| "unknown".to_string());
        let size = file
            .get("size")
            .and_then(|value| value.as_u64())
            .map(|value| format!("{value} bytes"))
            .unwrap_or_else(|| "unknown size".to_string());
        format!("[ATTACHMENT:{file_name} | mime={mime} | size={size}]")
    }

    fn extract_channel_ids(list_payload: &serde_json::Value) -> Vec<String> {
        let mut ids = list_payload
            .get("channels")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
            .filter_map(|channel| {
                let id = channel.get("id").and_then(|id| id.as_str())?;
                let is_archived = channel
                    .get("is_archived")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let is_member = channel
                    .get("is_member")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                if is_archived || !is_member {
                    return None;
                }
                Some(id.to_string())
            })
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        ids
    }

    async fn list_accessible_channels(&self) -> anyhow::Result<Vec<String>> {
        let mut channels = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut query_params = vec![
                ("exclude_archived", "true".to_string()),
                ("limit", "200".to_string()),
                (
                    "types",
                    "public_channel,private_channel,mpim,im".to_string(),
                ),
            ];
            if let Some(ref next) = cursor {
                query_params.push(("cursor", next.clone()));
            }

            let resp = self
                .http_client()
                .get("https://slack.com/api/conversations.list")
                .bearer_auth(&self.bot_token)
                .query(&query_params)
                .send()
                .await?;

            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

            if !status.is_success() {
                let sanitized = crate::providers::sanitize_api_error(&body);
                anyhow::bail!("Slack conversations.list failed ({status}): {sanitized}");
            }

            let data: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            if data.get("ok") == Some(&serde_json::Value::Bool(false)) {
                let err = data
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown");
                anyhow::bail!("Slack conversations.list failed: {err}");
            }

            channels.extend(Self::extract_channel_ids(&data));

            cursor = data
                .get("response_metadata")
                .and_then(|rm| rm.get("next_cursor"))
                .and_then(|c| c.as_str())
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(ToOwned::to_owned);

            if cursor.is_none() {
                break;
            }
        }

        channels.sort();
        channels.dedup();
        Ok(channels)
    }

    fn slack_now_ts() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}.{:06}", now.as_secs(), now.subsec_micros())
    }

    fn ensure_poll_cursor(
        cursors: &mut HashMap<String, String>,
        channel_id: &str,
        now_ts: &str,
    ) -> String {
        cursors
            .entry(channel_id.to_string())
            .or_insert_with(|| now_ts.to_string())
            .clone()
    }

    fn parse_block_action_as_command(
        envelope: &serde_json::Value,
        _bot_user_id: &str,
    ) -> Option<ChannelMessage> {
        let payload = envelope.get("payload")?;

        let payload_type = payload.get("type").and_then(|v| v.as_str())?;
        if payload_type != "block_actions" {
            return None;
        }

        let actions = payload.get("actions").and_then(|v| v.as_array())?;
        let action = actions.first()?;

        let action_id = action.get("action_id").and_then(|v| v.as_str())?;
        let selected_value = action
            .get("selected_option")
            .and_then(|o| o.get("value"))
            .and_then(|v| v.as_str())?;

        let command = match action_id {
            "sen_config_provider" => format!("/models {selected_value}"),
            "sen_config_model" => format!("/model {selected_value}"),
            _ => return None,
        };

        let user = payload
            .get("user")
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let channel_id = payload
            .get("channel")
            .and_then(|c| c.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if channel_id.is_empty() {
            tracing::warn!("Slack block_actions: missing channel ID in interactive payload");
            return None;
        }

        let ts = payload
            .get("message")
            .and_then(|m| m.get("ts"))
            .and_then(|v| v.as_str())
            .unwrap_or("0");

        Some(ChannelMessage {
            id: format!("slack_{channel_id}_{ts}_action"),
            sender: user.to_string(),
            reply_target: channel_id.to_string(),
            content: command,
            channel: "slack".to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: payload
                .get("message")
                .and_then(|m| m.get("thread_ts"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            interruption_scope_id: None,
            attachments: vec![],
        })
    }

    async fn open_socket_mode_url(&self) -> anyhow::Result<String> {
        let app_token = self
            .configured_app_token()
            .ok_or_else(|| anyhow::anyhow!("Slack Socket Mode requires app_token"))?;

        let resp = self
            .http_client()
            .post("https://slack.com/api/apps.connections.open")
            .bearer_auth(app_token)
            .send()
            .await?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&body);
            anyhow::bail!("Slack apps.connections.open failed ({status}): {sanitized}");
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack apps.connections.open failed: {err}");
        }

        parsed
            .get("url")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Slack apps.connections.open did not return url"))
    }

    async fn listen_socket_mode(
        &self,
        tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        bot_user_id: &str,
        scoped_channels: Option<Vec<String>>,
    ) -> anyhow::Result<()> {
        let mut last_ts_by_channel: HashMap<String, String> = HashMap::new();
        let mut open_url_attempt: u32 = 0;
        let mut socket_reconnect_attempt: u32 = 0;

        loop {
            let ws_url = match self.open_socket_mode_url().await {
                Ok(url) => {
                    open_url_attempt = 0;
                    url
                }
                Err(e) => {
                    let wait = Self::compute_socket_mode_retry_delay(open_url_attempt);
                    tracing::warn!(
                        "Slack Socket Mode: failed to open websocket URL: {e}; retrying in {:.3}s (attempt #{})",
                        wait.as_secs_f64(),
                        open_url_attempt.saturating_add(1),
                    );
                    open_url_attempt = open_url_attempt.saturating_add(1);
                    tokio::time::sleep(wait).await;
                    continue;
                }
            };

            let (ws_stream, _) = match crate::services::require_services()
                .proxy_runtime()
                .ws_connect(&ws_url, "channel.slack", self.proxy_url.as_deref())
                .await
            {
                Ok(connection) => {
                    socket_reconnect_attempt = 0;
                    connection
                }
                Err(e) => {
                    let wait = Self::compute_socket_mode_retry_delay(socket_reconnect_attempt);
                    tracing::warn!(
                        "Slack Socket Mode: websocket connect failed: {e}; retrying in {:.3}s (attempt #{})",
                        wait.as_secs_f64(),
                        socket_reconnect_attempt.saturating_add(1),
                    );
                    socket_reconnect_attempt = socket_reconnect_attempt.saturating_add(1);
                    tokio::time::sleep(wait).await;
                    continue;
                }
            };
            tracing::info!("Slack Socket Mode: websocket connected");

            let (mut write, mut read) = ws_stream.split();

            while let Some(frame) = read.next().await {
                let text = match frame {
                    Ok(WsMessage::Text(text)) => text,
                    Ok(WsMessage::Ping(payload)) => {
                        if let Err(e) = write.send(WsMessage::Pong(payload)).await {
                            tracing::warn!("Slack Socket Mode: pong send failed: {e}");
                            break;
                        }
                        continue;
                    }
                    Ok(WsMessage::Close(_)) => {
                        tracing::warn!("Slack Socket Mode: websocket closed by server");
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        tracing::warn!("Slack Socket Mode: websocket read failed: {e}");
                        break;
                    }
                };

                let envelope: serde_json::Value = match serde_json::from_str(text.as_ref()) {
                    Ok(value) => value,
                    Err(e) => {
                        tracing::warn!("Slack Socket Mode: invalid JSON payload: {e}");
                        continue;
                    }
                };

                let envelope_id = envelope
                    .get("envelope_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);

                macro_rules! ack_envelope {
                    () => {{
                        if let Some(ref eid) = envelope_id {
                            let ack = serde_json::json!({ "envelope_id": eid });
                            if let Err(e) =
                                write.send(WsMessage::Text(ack.to_string().into())).await
                            {
                                tracing::warn!("Slack Socket Mode: ack send failed: {e}");
                                break;
                            }
                        }
                    }};
                }

                let envelope_type = envelope
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if envelope_type == "disconnect" {
                    ack_envelope!();
                    tracing::warn!("Slack Socket Mode: received disconnect event");
                    break;
                }

                if envelope_type == "interactive" {
                    match Self::parse_block_action_as_command(&envelope, bot_user_id) {
                        Some(msg) => {
                            match crate::channels::forward_channel_message("slack", &tx, msg) {
                                crate::channels::ForwardOutcome::Delivered => ack_envelope!(),
                                crate::channels::ForwardOutcome::Dropped => break,
                                crate::channels::ForwardOutcome::Closed => return Ok(()),
                            }
                        }
                        None => ack_envelope!(),
                    }
                    continue;
                }

                if envelope_type != "events_api" {
                    ack_envelope!();
                    continue;
                }

                let Some(event) = envelope
                    .get("payload")
                    .and_then(|payload| payload.get("event"))
                else {
                    ack_envelope!();
                    continue;
                };
                let event_type = event
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                if event_type == "assistant_thread_started"
                    || event_type == "assistant_thread_context_changed"
                {
                    if let Some(thread) = event.get("assistant_thread") {
                        let ch = thread
                            .get("channel_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        let tts = thread
                            .get("thread_ts")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        if !ch.is_empty() && !tts.is_empty() {
                            {
                                let mut map = self.active_assistant_thread.lock();
                                map.insert(ch.to_string(), tts.to_string());
                            }
                        }
                    }
                    ack_envelope!();
                    continue;
                }

                if event_type != "message" {
                    ack_envelope!();
                    continue;
                }
                let subtype = event.get("subtype").and_then(|v| v.as_str());
                if !Self::is_supported_message_subtype(subtype) {
                    ack_envelope!();
                    continue;
                }

                let channel_id = event
                    .get("channel")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_default();
                if channel_id.is_empty() {
                    ack_envelope!();
                    continue;
                }
                if let Some(ref configured_channels) = scoped_channels {
                    if !configured_channels.iter().any(|id| id == &channel_id) {
                        ack_envelope!();
                        continue;
                    }
                }

                let user = event
                    .get("user")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if user.is_empty() || user == bot_user_id {
                    ack_envelope!();
                    continue;
                }
                if !self.is_user_allowed(user) {
                    tracing::warn!("Slack: ignoring message from unauthorized user: {user}");
                    ack_envelope!();
                    continue;
                }

                let ts = event.get("ts").and_then(|v| v.as_str()).unwrap_or_default();
                if ts.is_empty() {
                    ack_envelope!();
                    continue;
                }
                let last_ts = last_ts_by_channel
                    .get(&channel_id)
                    .map(String::as_str)
                    .unwrap_or_default();
                if ts <= last_ts {
                    ack_envelope!();
                    continue;
                }

                let is_group_message = Self::is_group_channel_id(&channel_id);
                let is_thread_reply = event.get("thread_ts").and_then(|v| v.as_str()).is_some();
                let allow_sender_without_mention =
                    is_group_message && self.is_group_sender_trigger_enabled(user);
                let require_mention = self.mention_only
                    && is_group_message
                    && !allow_sender_without_mention
                    && !is_thread_reply;

                let Some(normalized_text) = self
                    .build_incoming_content(event, require_mention, bot_user_id)
                    .await
                else {
                    ack_envelope!();
                    continue;
                };

                let sender = self.resolve_sender_identity(user).await;

                let channel_msg = ChannelMessage {
                    id: format!("slack_{channel_id}_{ts}"),
                    sender,
                    reply_target: channel_id.clone(),
                    content: normalized_text,
                    channel: "slack".to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    thread_ts: if self.thread_replies {
                        Self::inbound_thread_ts(event, ts)
                    } else {
                        Self::inbound_thread_ts_genuine_only(event)
                    },
                    interruption_scope_id: Self::inbound_interruption_scope_id(event, ts),
                    attachments: vec![],
                };

                if let Some(ref tts) = channel_msg.thread_ts {
                    let mut map = self.active_assistant_thread.lock();
                    map.insert(channel_id.clone(), tts.clone());
                }

                match crate::channels::forward_channel_message("slack", &tx, channel_msg) {
                    crate::channels::ForwardOutcome::Delivered => {
                        last_ts_by_channel.insert(channel_id.clone(), ts.to_string());
                        ack_envelope!();
                    }
                    crate::channels::ForwardOutcome::Dropped => break,
                    crate::channels::ForwardOutcome::Closed => return Ok(()),
                }
            }

            let wait = Self::compute_socket_mode_retry_delay(socket_reconnect_attempt);
            tracing::warn!(
                "Slack Socket Mode: reconnecting in {:.3}s (attempt #{})...",
                wait.as_secs_f64(),
                socket_reconnect_attempt.saturating_add(1),
            );
            socket_reconnect_attempt = socket_reconnect_attempt.saturating_add(1);
            tokio::time::sleep(wait).await;
        }
    }

    fn parse_retry_after_secs(headers: &HeaderMap) -> Option<u64> {
        let value = headers
            .get(reqwest::header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .trim();
        Self::parse_retry_after_value(value)
    }

    fn parse_retry_after_value(value: &str) -> Option<u64> {
        if value.is_empty() {
            return None;
        }

        if let Ok(seconds) = value.parse::<u64>() {
            return Some(seconds);
        }

        let truncated = value
            .split_once('.')
            .map(|(whole, _)| whole)
            .unwrap_or(value);
        truncated.parse::<u64>().ok()
    }

    fn jitter_ms(max_jitter_ms: u64) -> u64 {
        if max_jitter_ms == 0 {
            return 0;
        }
        rand::random::<u64>() % (max_jitter_ms + 1)
    }

    fn compute_exponential_backoff_delay(
        base_retry_after_secs: u64,
        attempt: u32,
        max_backoff_secs: u64,
        jitter_ms: u64,
    ) -> Duration {
        let multiplier = 1_u64.checked_shl(attempt).unwrap_or(u64::MAX);
        let backoff_secs = base_retry_after_secs
            .saturating_mul(multiplier)
            .min(max_backoff_secs);
        Duration::from_secs(backoff_secs) + Duration::from_millis(jitter_ms)
    }

    fn compute_retry_delay(base_retry_after_secs: u64, attempt: u32, jitter_ms: u64) -> Duration {
        Self::compute_exponential_backoff_delay(
            base_retry_after_secs,
            attempt,
            SLACK_HISTORY_MAX_BACKOFF_SECS,
            jitter_ms,
        )
    }

    fn compute_socket_mode_retry_delay(attempt: u32) -> Duration {
        let jitter_ms = Self::jitter_ms(SLACK_SOCKET_MODE_MAX_JITTER_MS);
        Self::compute_exponential_backoff_delay(
            SLACK_SOCKET_MODE_INITIAL_BACKOFF_SECS,
            attempt,
            SLACK_SOCKET_MODE_MAX_BACKOFF_SECS,
            jitter_ms,
        )
    }

    fn next_retry_timestamp(wait: Duration) -> String {
        match chrono::Duration::from_std(wait) {
            Ok(delta) => (Utc::now() + delta).to_rfc3339(),
            Err(_) => Utc::now().to_rfc3339(),
        }
    }

    fn evaluate_health(bot_ok: bool, socket_mode_enabled: bool, socket_mode_ok: bool) -> bool {
        if !bot_ok {
            return false;
        }
        if socket_mode_enabled {
            return socket_mode_ok;
        }
        true
    }

    fn slack_api_call_succeeded(status: reqwest::StatusCode, body: &str) -> bool {
        if !status.is_success() {
            return false;
        }

        let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        parsed
            .get("ok")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    async fn fetch_history_with_retry(
        &self,
        channel_id: &str,
        params: &[(&str, String)],
    ) -> Option<serde_json::Value> {
        let mut total_wait = Duration::from_secs(0);

        for attempt in 0..=SLACK_HISTORY_MAX_RETRIES {
            let resp = match self
                .http_client()
                .get("https://slack.com/api/conversations.history")
                .bearer_auth(&self.bot_token)
                .query(params)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Slack poll error for channel {channel_id}: {e}");
                    return None;
                }
            };

            let status = resp.status();
            let headers = resp.headers().clone();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

            let is_ratelimited_http = status == reqwest::StatusCode::TOO_MANY_REQUESTS;
            let payload: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let is_ratelimited_payload = payload.get("ok") == Some(&serde_json::Value::Bool(false))
                && payload
                    .get("error")
                    .and_then(|e| e.as_str())
                    .is_some_and(|err| err == "ratelimited");

            if is_ratelimited_http || is_ratelimited_payload {
                if attempt >= SLACK_HISTORY_MAX_RETRIES {
                    tracing::error!(
                        "Slack rate limit retries exhausted for conversations.history on channel {}. Total wait: {}s across {} attempts. Proceeding without channel history.",
                        channel_id,
                        total_wait.as_secs(),
                        SLACK_HISTORY_MAX_RETRIES
                    );
                    return None;
                }

                let retry_after_secs = Self::parse_retry_after_secs(&headers)
                    .unwrap_or(SLACK_HISTORY_DEFAULT_RETRY_AFTER_SECS);
                let jitter_ms = Self::jitter_ms(SLACK_HISTORY_MAX_JITTER_MS);
                let wait = Self::compute_retry_delay(retry_after_secs, attempt, jitter_ms);
                total_wait += wait;
                let next_retry_at = Self::next_retry_timestamp(wait);
                tracing::warn!(
                    "Slack conversations.history rate limited for channel {}. Retry-After: {}s. Attempt {}/{}. Next retry at {}.",
                    channel_id,
                    retry_after_secs,
                    attempt + 1,
                    SLACK_HISTORY_MAX_RETRIES,
                    next_retry_at
                );
                tokio::time::sleep(wait).await;
                continue;
            }

            if !status.is_success() {
                let sanitized = crate::providers::sanitize_api_error(&body);
                tracing::warn!(
                    "Slack history request failed for channel {} ({}): {}",
                    channel_id,
                    status,
                    sanitized
                );
                return None;
            }

            if payload.get("ok") == Some(&serde_json::Value::Bool(false)) {
                let err = payload
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown");
                tracing::warn!("Slack history error for channel {channel_id}: {err}");
                return None;
            }

            return Some(payload);
        }

        None
    }

    async fn fetch_thread_replies_with_retry(
        &self,
        channel_id: &str,
        thread_ts: &str,
        oldest: &str,
    ) -> Option<serde_json::Value> {
        let mut total_wait = Duration::from_secs(0);

        for attempt in 0..=SLACK_HISTORY_MAX_RETRIES {
            let resp = match self
                .http_client()
                .get("https://slack.com/api/conversations.replies")
                .bearer_auth(&self.bot_token)
                .query(&[
                    ("channel", channel_id),
                    ("ts", thread_ts),
                    ("oldest", oldest),
                    ("limit", "50"),
                ])
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Slack conversations.replies error for thread {thread_ts} in {channel_id}: {e}"
                    );
                    return None;
                }
            };

            let status = resp.status();
            let headers = resp.headers().clone();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

            let is_ratelimited_http = status == reqwest::StatusCode::TOO_MANY_REQUESTS;
            let payload: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let is_ratelimited_payload = payload.get("ok") == Some(&serde_json::Value::Bool(false))
                && payload
                    .get("error")
                    .and_then(|e| e.as_str())
                    .is_some_and(|err| err == "ratelimited");

            if is_ratelimited_http || is_ratelimited_payload {
                if attempt >= SLACK_HISTORY_MAX_RETRIES {
                    tracing::error!(
                        "Slack rate limit retries exhausted for conversations.replies on thread {} in channel {}. Total wait: {}s across {} attempts.",
                        thread_ts,
                        channel_id,
                        total_wait.as_secs(),
                        SLACK_HISTORY_MAX_RETRIES
                    );
                    return None;
                }

                let retry_after_secs = Self::parse_retry_after_secs(&headers)
                    .unwrap_or(SLACK_HISTORY_DEFAULT_RETRY_AFTER_SECS);
                let jitter_ms = Self::jitter_ms(SLACK_HISTORY_MAX_JITTER_MS);
                let wait = Self::compute_retry_delay(retry_after_secs, attempt, jitter_ms);
                total_wait += wait;
                let next_retry_at = Self::next_retry_timestamp(wait);
                tracing::warn!(
                    "Slack conversations.replies rate limited for thread {} in channel {}. Retry-After: {}s. Attempt {}/{}. Next retry at {}.",
                    thread_ts,
                    channel_id,
                    retry_after_secs,
                    attempt + 1,
                    SLACK_HISTORY_MAX_RETRIES,
                    next_retry_at
                );
                tokio::time::sleep(wait).await;
                continue;
            }

            if !status.is_success() {
                let sanitized = crate::providers::sanitize_api_error(&body);
                tracing::warn!(
                    "Slack conversations.replies failed for thread {} in channel {} ({}): {}",
                    thread_ts,
                    channel_id,
                    status,
                    sanitized
                );
                return None;
            }

            if payload.get("ok") == Some(&serde_json::Value::Bool(false)) {
                let err = payload
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown");
                tracing::warn!(
                    "Slack conversations.replies error for thread {} in channel {}: {}",
                    thread_ts,
                    channel_id,
                    err
                );
                return None;
            }

            return Some(payload);
        }

        None
    }

    fn extract_active_threads(messages: &[serde_json::Value]) -> Vec<(String, String)> {
        messages
            .iter()
            .filter_map(|msg| {
                let thread_ts = msg.get("thread_ts").and_then(|v| v.as_str())?;
                let ts = msg.get("ts").and_then(|v| v.as_str()).unwrap_or_default();

                if ts != thread_ts {
                    return None;
                }
                let reply_count = msg.get("reply_count").and_then(|v| v.as_u64()).unwrap_or(0);
                if reply_count == 0 {
                    return None;
                }
                let latest_reply = msg
                    .get("latest_reply")
                    .and_then(|v| v.as_str())
                    .unwrap_or(thread_ts);
                Some((thread_ts.to_string(), latest_reply.to_string()))
            })
            .collect()
    }

    fn evict_stale_threads(
        active_threads: &mut HashMap<String, (String, String, Instant)>,
        now: Instant,
    ) {
        let max_age = Duration::from_secs(SLACK_POLL_THREAD_EXPIRE_SECS);
        active_threads
            .retain(|_, (_, _, last_activity)| now.duration_since(*last_activity) < max_age);
        if active_threads.len() > SLACK_POLL_ACTIVE_THREAD_MAX {
            let overflow = active_threads.len() - SLACK_POLL_ACTIVE_THREAD_MAX;
            let mut entries: Vec<_> = active_threads
                .iter()
                .map(|(k, (_, _, t))| (k.clone(), *t))
                .collect();
            entries.sort_by_key(|(_, t)| *t);
            for (key, _) in entries.into_iter().take(overflow) {
                active_threads.remove(&key);
            }
        }
    }
}

const SLACK_TRUNCATION_INDICATOR: &str = "\n\n...[message truncated]";

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn split_text_into_chunks(text: &str, max_chars: usize, max_chunks: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() && chunks.len() < max_chunks {
        let is_last_slot = chunks.len() + 1 == max_chunks;

        if remaining.len() <= max_chars && !is_last_slot {
            chunks.push(remaining.to_string());
            break;
        }

        if is_last_slot {

            if remaining.len() <= max_chars {
                chunks.push(remaining.to_string());
            } else {

                let avail = floor_char_boundary(
                    remaining,
                    max_chars.saturating_sub(SLACK_TRUNCATION_INDICATOR.len()),
                );
                let break_at = remaining[..avail]
                    .rfind('\n')
                    .map(|i| i + 1)
                    .or_else(|| remaining[..avail].rfind(' ').map(|i| i + 1))
                    .unwrap_or(avail);
                let mut chunk = remaining[..break_at].to_string();
                chunk.push_str(SLACK_TRUNCATION_INDICATOR);
                chunks.push(chunk);
            }
            break;
        }

        let limit = floor_char_boundary(remaining, max_chars.min(remaining.len()));
        let mut break_at = remaining[..limit]
            .rfind('\n')
            .map(|i| i + 1)
            .or_else(|| remaining[..limit].rfind(' ').map(|i| i + 1))
            .unwrap_or(limit);

        if break_at == 0 {
            break_at = remaining
                .char_indices()
                .nth(1)
                .map_or(remaining.len(), |(i, _)| i);
        }

        chunks.push(remaining[..break_at].to_string());
        remaining = &remaining[break_at..];
    }

    chunks
}

#[async_trait]
impl Channel for SlackChannel {
    fn name(&self) -> &str {
        "slack"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        let body = if let Some(blocks_json) = message.content.strip_prefix(super::BLOCK_KIT_PREFIX)
        {
            let blocks: serde_json::Value = serde_json::from_str(blocks_json)
                .context("invalid Block Kit JSON in runtime command response")?;
            let mut body = serde_json::json!({
                "channel": message.recipient,
                "text": "Model configuration",
                "blocks": blocks
            });
            if let Some(ts) = self.outbound_thread_ts(message) {
                body["thread_ts"] = serde_json::json!(ts);
            }
            body
        } else {
            let mut body = serde_json::json!({
                "channel": message.recipient,
                "text": message.content
            });

            let block_limit = if self.use_markdown_blocks {
                SLACK_MARKDOWN_BLOCK_MAX_CHARS
            } else {
                SLACK_BLOCK_TEXT_MAX_CHARS
            };
            if message.content.len() <= SLACK_MARKDOWN_BLOCK_MAX_CHARS {
                let chunks = split_text_into_chunks(
                    &message.content,
                    block_limit,
                    SLACK_MAX_BLOCKS_PER_MESSAGE,
                );
                let blocks: Vec<serde_json::Value> = chunks
                    .into_iter()
                    .map(|chunk| {
                        if self.use_markdown_blocks {
                            serde_json::json!({
                                "type": "markdown",
                                "text": chunk
                            })
                        } else {
                            serde_json::json!({
                                "type": "section",
                                "text": {
                                    "type": "mrkdwn",
                                    "text": chunk
                                }
                            })
                        }
                    })
                    .collect();
                body["blocks"] = serde_json::Value::Array(blocks);
            }

            if let Some(ts) = self.outbound_thread_ts(message) {
                body["thread_ts"] = serde_json::json!(ts);
            }
            body
        };

        let resp = self
            .http_client()
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&body);
            anyhow::bail!("Slack chat.postMessage failed ({status}): {sanitized}");
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack chat.postMessage failed: {err}");
        }

        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        self.stream_drafts
    }

    async fn send_draft(&self, message: &SendMessage) -> anyhow::Result<Option<String>> {
        if !self.stream_drafts {
            return Ok(None);
        }

        let thread_ts = self.outbound_thread_ts(message).unwrap_or_default();
        let lazy_id = format!("{LAZY_DRAFT_PREFIX}{}:{}", message.recipient, thread_ts);
        Ok(Some(lazy_id))
    }

    async fn update_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {

        if message_id.starts_with(LAZY_DRAFT_PREFIX)
            && self.resolve_draft_ts(message_id).await.is_none()
        {

            let _ = self.materialize_lazy_draft(message_id, text).await;
            self.last_draft_edit
                .lock()
                .insert(recipient.to_string(), Instant::now());
            return Ok(());
        }

        let real_ts = match self.resolve_draft_ts(message_id).await {
            Some(ts) => ts,
            None => return Ok(()),
        };

        {
            let mut last_edits = self.last_draft_edit.lock();

            let ttl = std::time::Duration::from_secs(3600);
            last_edits.retain(|_, ts| ts.elapsed() < ttl);

            if let Some(last_time) = last_edits.get(recipient) {
                let elapsed_ms = u64::try_from(last_time.elapsed().as_millis()).unwrap_or(u64::MAX);
                if elapsed_ms < self.draft_update_interval_ms {
                    return Ok(());
                }
            }
        }

        self.last_draft_edit
            .lock()
            .insert(recipient.to_string(), Instant::now());

        let display_text = if text.len() > SLACK_MESSAGE_MAX_CHARS {
            text[..text
                .char_indices()
                .take_while(|(idx, _)| *idx < SLACK_MESSAGE_MAX_CHARS)
                .last()
                .map_or(0, |(idx, ch)| idx + ch.len_utf8())]
                .to_string()
        } else {
            text.to_string()
        };

        let client = self.http_client();
        let token = self.bot_token.clone();
        let channel = recipient.to_string();
        let _update_task =
            crate::runtime::spawn_supervised("channels.slack.edit_sync", async move {
                let mut body = serde_json::json!({
                    "channel": channel,
                    "ts": real_ts,
                    "text": &display_text,
                });
                if display_text.len() <= SLACK_MARKDOWN_BLOCK_MAX_CHARS {
                    body["blocks"] = serde_json::json!([{
                        "type": "markdown",
                        "text": &display_text
                    }]);
                }
                match client
                    .post("https://slack.com/api/chat.update")
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if let Ok(resp_body) = resp.json::<serde_json::Value>().await {
                            if resp_body.get("ok") != Some(&serde_json::Value::Bool(true)) {
                                let err = resp_body
                                    .get("error")
                                    .and_then(|e| e.as_str())
                                    .unwrap_or("unknown");
                                tracing::debug!("Slack chat.update (draft) failed: {err}");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Slack chat.update (draft) HTTP error: {e}");
                    }
                }
            });

        Ok(())
    }

    async fn update_draft_progress(
        &self,
        recipient: &str,
        _message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let status_line = text.trim().lines().last().unwrap_or("").trim();

        if status_line.is_empty() || status_line.starts_with("\u{1f914}") {
            return Ok(());
        }
        self.set_assistant_status(recipient, status_line).await;
        Ok(())
    }

    async fn finalize_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {

        self.last_draft_edit.lock().remove(recipient);

        let real_ts = self.resolve_draft_ts(message_id).await;

        self.lazy_draft_ts.lock().await.remove(message_id);

        let Some(real_ts) = real_ts else {

            return self.send(&SendMessage::new(text, recipient)).await;
        };

        if text.len() > SLACK_MESSAGE_MAX_CHARS {
            let _ = self.delete_message(recipient, &real_ts).await;
            return self.send(&SendMessage::new(text, recipient)).await;
        }

        let mut body = serde_json::json!({
            "channel": recipient,
            "ts": real_ts,
            "text": text,
        });

        if text.len() <= SLACK_MARKDOWN_BLOCK_MAX_CHARS {
            body["blocks"] = serde_json::json!([{
                "type": "markdown",
                "text": text
            }]);
        }

        let resp = self
            .http_client()
            .post("https://slack.com/api/chat.update")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let resp_body: serde_json::Value = resp.json().await?;
        if resp_body.get("ok") == Some(&serde_json::Value::Bool(true)) {
            return Ok(());
        }

        let err = resp_body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown");
        tracing::debug!("Slack chat.update (finalize) failed: {err}; falling back to delete+send");

        let _ = self.delete_message(recipient, &real_ts).await;
        self.send(&SendMessage::new(text, recipient)).await
    }

    async fn cancel_draft(&self, recipient: &str, message_id: &str) -> anyhow::Result<()> {
        self.last_draft_edit.lock().remove(recipient);
        let real_ts = self.resolve_draft_ts(message_id).await;
        self.lazy_draft_ts.lock().await.remove(message_id);
        if let Some(ts) = real_ts {
            self.delete_message(recipient, &ts).await
        } else {
            Ok(())
        }
    }

    async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let ts = extract_slack_ts(message_id);
        let name = unicode_emoji_to_slack_name(emoji);

        let body = serde_json::json!({
            "channel": channel_id,
            "timestamp": ts,
            "name": name
        });

        let resp = self
            .http_client()
            .post("https://slack.com/api/reactions.add")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&text);
            anyhow::bail!("Slack reactions.add failed ({status}): {sanitized}");
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            if err != "already_reacted" {
                anyhow::bail!("Slack reactions.add failed: {err}");
            }
        }

        Ok(())
    }

    async fn remove_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let ts = extract_slack_ts(message_id);
        let name = unicode_emoji_to_slack_name(emoji);

        let body = serde_json::json!({
            "channel": channel_id,
            "timestamp": ts,
            "name": name
        });

        let resp = self
            .http_client()
            .post("https://slack.com/api/reactions.remove")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            let sanitized = crate::providers::sanitize_api_error(&text);
            anyhow::bail!("Slack reactions.remove failed ({status}): {sanitized}");
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            if err != "no_reaction" {
                anyhow::bail!("Slack reactions.remove failed: {err}");
            }
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
        let scoped_channels = self.scoped_channel_ids();
        if self.configured_app_token().is_some() {
            tracing::info!("Slack channel listening in Socket Mode");
            return self
                .listen_socket_mode(tx, &bot_user_id, scoped_channels)
                .await;
        }

        let mut discovered_channels: Vec<String> = Vec::new();
        let mut last_discovery = Instant::now();
        let mut last_ts_by_channel: HashMap<String, String> = HashMap::new();

        let mut active_threads: HashMap<String, (String, String, Instant)> = HashMap::new();

        if let Some(ref channel_ids) = scoped_channels {
            tracing::info!(
                "Slack channel listening on {} configured channel(s): {}",
                channel_ids.len(),
                channel_ids.join(", ")
            );
        } else {
            tracing::info!(
                "Slack channel_id/channel_ids not set (or wildcard only); listening across all accessible channels."
            );
        }

        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;

            let target_channels = if let Some(ref channel_ids) = scoped_channels {
                channel_ids.clone()
            } else {
                if discovered_channels.is_empty()
                    || last_discovery.elapsed() >= Duration::from_secs(60)
                {
                    match self.list_accessible_channels().await {
                        Ok(channels) => {
                            if channels != discovered_channels {
                                tracing::info!(
                                    "Slack auto-discovery refreshed: listening on {} channel(s).",
                                    channels.len()
                                );
                            }
                            discovered_channels = channels;
                        }
                        Err(e) => {
                            tracing::warn!("Slack channel discovery failed: {e}");
                        }
                    }
                    last_discovery = Instant::now();
                }

                discovered_channels.clone()
            };

            if target_channels.is_empty() {
                tracing::debug!("Slack: no accessible channels discovered yet");
                continue;
            }

            for channel_id in target_channels {
                let had_cursor = last_ts_by_channel.contains_key(&channel_id);
                let bootstrap_ts = Self::slack_now_ts();
                let cursor_ts =
                    Self::ensure_poll_cursor(&mut last_ts_by_channel, &channel_id, &bootstrap_ts);
                if !had_cursor {
                    tracing::debug!(
                        "Slack: initialized cursor for channel {} at {} to prevent historical replay",
                        channel_id,
                        cursor_ts
                    );
                }
                let params = vec![
                    ("channel", channel_id.clone()),
                    ("limit", "10".to_string()),
                    ("oldest", cursor_ts),
                ];

                let Some(data) = self.fetch_history_with_retry(&channel_id, &params).await else {
                    continue;
                };

                if let Some(messages) = data.get("messages").and_then(|m| m.as_array()) {

                    for (thread_ts, latest_reply) in Self::extract_active_threads(messages) {
                        let entry = active_threads.entry(thread_ts.clone()).or_insert_with(|| {
                            (channel_id.clone(), thread_ts.clone(), Instant::now())
                        });
                        if latest_reply > entry.1 {
                            entry.1 = latest_reply;
                        }
                        entry.2 = Instant::now();
                    }

                    for msg in messages.iter().rev() {
                        let subtype = msg.get("subtype").and_then(|value| value.as_str());
                        if !Self::is_supported_message_subtype(subtype) {
                            continue;
                        }
                        let ts = msg.get("ts").and_then(|t| t.as_str()).unwrap_or("");
                        let user = msg
                            .get("user")
                            .and_then(|u| u.as_str())
                            .unwrap_or("unknown");
                        let last_ts = last_ts_by_channel
                            .get(&channel_id)
                            .map(String::as_str)
                            .unwrap_or("");

                        if user == bot_user_id {
                            continue;
                        }

                        if !self.is_user_allowed(user) {
                            tracing::warn!(
                                "Slack: ignoring message from unauthorized user: {user}"
                            );
                            continue;
                        }

                        if ts <= last_ts {
                            continue;
                        }

                        let is_group_message = Self::is_group_channel_id(&channel_id);
                        let is_thread_reply =
                            msg.get("thread_ts").and_then(|v| v.as_str()).is_some();
                        let allow_sender_without_mention =
                            is_group_message && self.is_group_sender_trigger_enabled(user);
                        let require_mention = self.mention_only
                            && is_group_message
                            && !allow_sender_without_mention
                            && !is_thread_reply;
                        let Some(normalized_text) = self
                            .build_incoming_content(msg, require_mention, &bot_user_id)
                            .await
                        else {
                            continue;
                        };

                        let sender = self.resolve_sender_identity(user).await;

                        let channel_msg = ChannelMessage {
                            id: format!("slack_{channel_id}_{ts}"),
                            sender,
                            reply_target: channel_id.clone(),
                            content: normalized_text,
                            channel: "slack".to_string(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            thread_ts: if self.thread_replies {
                                Self::inbound_thread_ts(msg, ts)
                            } else {
                                Self::inbound_thread_ts_genuine_only(msg)
                            },
                            interruption_scope_id: Self::inbound_interruption_scope_id(msg, ts),
                            attachments: vec![],
                        };

                        match crate::channels::forward_channel_message("slack", &tx, channel_msg) {
                            crate::channels::ForwardOutcome::Delivered => {
                                last_ts_by_channel.insert(channel_id.clone(), ts.to_string());
                            }
                            crate::channels::ForwardOutcome::Dropped => break,
                            crate::channels::ForwardOutcome::Closed => return Ok(()),
                        }
                    }
                }
            }

            Self::evict_stale_threads(&mut active_threads, Instant::now());
            let thread_snapshot: Vec<(String, String, String)> = active_threads
                .iter()
                .map(|(thread_ts, (ch, last_reply, _))| {
                    (thread_ts.clone(), ch.clone(), last_reply.clone())
                })
                .collect();

            for (thread_ts, thread_channel_id, last_reply_ts) in thread_snapshot {
                let Some(data) = self
                    .fetch_thread_replies_with_retry(&thread_channel_id, &thread_ts, &last_reply_ts)
                    .await
                else {
                    continue;
                };

                let Some(replies) = data.get("messages").and_then(|m| m.as_array()) else {
                    continue;
                };

                for reply in replies {
                    let reply_ts = reply.get("ts").and_then(|v| v.as_str()).unwrap_or_default();
                    if reply_ts.is_empty() || reply_ts <= last_reply_ts.as_str() {
                        continue;
                    }
                    let subtype = reply.get("subtype").and_then(|v| v.as_str());
                    if !Self::is_supported_message_subtype(subtype) {
                        continue;
                    }

                    let user = reply
                        .get("user")
                        .and_then(|u| u.as_str())
                        .unwrap_or_default();
                    if user.is_empty() || user == bot_user_id {
                        continue;
                    }
                    if !self.is_user_allowed(user) {
                        continue;
                    }

                    let require_mention = false;
                    let Some(normalized_text) = self
                        .build_incoming_content(reply, require_mention, &bot_user_id)
                        .await
                    else {
                        continue;
                    };

                    let sender = self.resolve_sender_identity(user).await;

                    let channel_msg = ChannelMessage {
                        id: format!("slack_{thread_channel_id}_{reply_ts}"),
                        sender,
                        reply_target: thread_channel_id.clone(),
                        content: normalized_text,
                        channel: "slack".to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        thread_ts: Some(thread_ts.clone()),
                        interruption_scope_id: Some(thread_ts.clone()),
                        attachments: vec![],
                    };

                    match crate::channels::forward_channel_message("slack", &tx, channel_msg) {
                        crate::channels::ForwardOutcome::Delivered => {
                            if let Some(entry) = active_threads.get_mut(&thread_ts) {
                                if reply_ts > entry.1.as_str() {
                                    entry.1 = reply_ts.to_string();
                                }
                                entry.2 = Instant::now();
                            }
                        }
                        crate::channels::ForwardOutcome::Dropped => break,
                        crate::channels::ForwardOutcome::Closed => return Ok(()),
                    }
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        let bot_ok = match self
            .http_client()
            .get("https://slack.com/api/auth.test")
            .bearer_auth(&self.bot_token)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                Self::slack_api_call_succeeded(status, &body)
            }
            Err(_) => false,
        };
        let socket_mode_enabled = self.configured_app_token().is_some();
        let socket_mode_ok = if socket_mode_enabled {
            self.open_socket_mode_url().await.is_ok()
        } else {
            true
        };
        Self::evaluate_health(bot_ok, socket_mode_enabled, socket_mode_ok)
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let thread_ts = {
            let map = self.active_assistant_thread.lock();
            match map.get(recipient) {
                Some(ts) => ts.clone(),
                None => return Ok(()),
            }
        };

        let body = serde_json::json!({
            "channel_id": recipient,
            "thread_ts": thread_ts,
            "status": "is thinking...",
        });

        if let Ok(resp) = self
            .http_client()
            .post("https://slack.com/api/assistant.threads.setStatus")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await
        {
            if !resp.status().is_success() {
                tracing::debug!(
                    "assistant.threads.setStatus returned {}; ignoring",
                    resp.status()
                );
            }
        }

        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> anyhow::Result<()> {

        if self.stream_drafts {
            self.set_assistant_status(recipient, "").await;
        }
        Ok(())
    }
}
