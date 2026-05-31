// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use matrix_sdk::{
    Client as MatrixSdkClient, LoopCtrl, Room, RoomState, SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    ruma::{
        OwnedEventId, OwnedRoomId, OwnedUserId,
        api::client::receipt::create_receipt,
        events::reaction::ReactionEventContent,
        events::receipt::ReceiptThread,
        events::relation::{Annotation, Thread},
        events::room::MediaSource,
        events::room::member::StrippedRoomMemberEvent,
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, Relation, ReplacementMetadata,
            RoomMessageEventContent,
        },
    },
};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, OnceCell, RwLock, mpsc};

#[derive(Clone)]
pub struct MatrixChannel {
    homeserver: String,
    access_token: String,
    room_id: String,
    allowed_users: Vec<String>,
    allowed_rooms: Vec<String>,
    session_owner_hint: Option<String>,
    session_device_id_hint: Option<String>,
    sen_dir: Option<PathBuf>,
    resolved_room_id_cache: Arc<RwLock<Option<String>>>,
    sdk_client: Arc<OnceCell<MatrixSdkClient>>,
    http_client: Client,
    reaction_events: Arc<RwLock<HashMap<String, String>>>,
    voice_mode: Arc<AtomicBool>,
    otk_conflict_detected: Arc<AtomicBool>,
    recovery_key: Option<String>,
    transcription: Option<crate::config::TranscriptionConfig>,
    transcription_manager: Option<Arc<super::pipeline::transcription::TranscriptionManager>>,
    stream_mode: crate::config::StreamMode,
    draft_update_interval_ms: u64,
    multi_message_delay_ms: u64,

    last_draft_edit: Arc<Mutex<HashMap<String, std::time::Instant>>>,

    multi_message_sent_len: Arc<Mutex<HashMap<String, usize>>>,

    multi_message_thread_ts: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl std::fmt::Debug for MatrixChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixChannel")
            .field("homeserver", &self.homeserver)
            .field("room_id", &self.room_id)
            .field("allowed_users", &self.allowed_users)
            .field("allowed_rooms", &self.allowed_rooms)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize)]
struct SyncResponse {
    next_batch: String,
    #[serde(default)]
    rooms: Rooms,
}

#[derive(Debug, Deserialize, Default)]
struct Rooms {
    #[serde(default)]
    join: std::collections::HashMap<String, JoinedRoom>,
}

#[derive(Debug, Deserialize)]
struct JoinedRoom {
    #[serde(default)]
    timeline: Timeline,
}

#[derive(Debug, Deserialize, Default)]
struct Timeline {
    #[serde(default)]
    events: Vec<TimelineEvent>,
}

#[derive(Debug, Deserialize)]
struct TimelineEvent {
    #[serde(rename = "type")]
    event_type: String,
    sender: String,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    content: EventContent,
}

#[derive(Debug, Deserialize, Default)]
struct EventContent {
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    msgtype: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WhoAmIResponse {
    user_id: String,
    #[serde(default)]
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RoomAliasResponse {
    room_id: String,
}

impl MatrixChannel {
    fn is_otk_conflict_message(message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        lower.contains("one time key") && lower.contains("already exists")
    }

    fn sanitize_error_for_log(error: &impl std::fmt::Display) -> String {
        crate::providers::sanitize_api_error(&error.to_string())
    }

    fn normalize_optional_field(value: Option<String>) -> Option<String> {
        value
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
    }

    pub fn new(
        homeserver: String,
        access_token: String,
        room_id: String,
        allowed_users: Vec<String>,
    ) -> Self {
        Self::new_full(
            homeserver,
            access_token,
            room_id,
            allowed_users,
            vec![],
            None,
            None,
            None,
            None,
        )
    }

    pub fn new_with_session_hint(
        homeserver: String,
        access_token: String,
        room_id: String,
        allowed_users: Vec<String>,
        owner_hint: Option<String>,
        device_id_hint: Option<String>,
    ) -> Self {
        Self::new_full(
            homeserver,
            access_token,
            room_id,
            allowed_users,
            vec![],
            owner_hint,
            device_id_hint,
            None,
            None,
        )
    }

    pub fn new_with_session_hint_and_sen_dir(
        homeserver: String,
        access_token: String,
        room_id: String,
        allowed_users: Vec<String>,
        owner_hint: Option<String>,
        device_id_hint: Option<String>,
        sen_dir: Option<PathBuf>,
    ) -> Self {
        Self::new_full(
            homeserver,
            access_token,
            room_id,
            allowed_users,
            vec![],
            owner_hint,
            device_id_hint,
            sen_dir,
            None,
        )
    }

    pub fn new_full(
        homeserver: String,
        access_token: String,
        room_id: String,
        allowed_users: Vec<String>,
        allowed_rooms: Vec<String>,
        owner_hint: Option<String>,
        device_id_hint: Option<String>,
        sen_dir: Option<PathBuf>,
        recovery_key: Option<String>,
    ) -> Self {
        let homeserver = homeserver.trim_end_matches('/').to_string();
        let access_token = access_token.trim().to_string();
        let room_id = room_id.trim().to_string();
        let allowed_users = allowed_users
            .into_iter()
            .map(|user| user.trim().to_string())
            .filter(|user| !user.is_empty())
            .collect();
        let allowed_rooms = allowed_rooms
            .into_iter()
            .map(|room| room.trim().to_string())
            .filter(|room| !room.is_empty())
            .collect();

        Self {
            homeserver,
            access_token,
            room_id,
            allowed_users,
            allowed_rooms,
            session_owner_hint: Self::normalize_optional_field(owner_hint),
            session_device_id_hint: Self::normalize_optional_field(device_id_hint),
            sen_dir,
            resolved_room_id_cache: Arc::new(RwLock::new(None)),
            sdk_client: Arc::new(OnceCell::new()),
            http_client: Client::new(),
            reaction_events: Arc::new(RwLock::new(HashMap::new())),
            voice_mode: Arc::new(AtomicBool::new(false)),
            otk_conflict_detected: Arc::new(AtomicBool::new(false)),
            recovery_key,
            transcription: None,
            transcription_manager: None,
            stream_mode: crate::config::StreamMode::Off,
            draft_update_interval_ms: 1500,
            multi_message_delay_ms: 800,
            last_draft_edit: Arc::new(Mutex::new(HashMap::new())),
            multi_message_sent_len: Arc::new(Mutex::new(HashMap::new())),
            multi_message_thread_ts: Arc::new(Mutex::new(HashMap::new())),
        }
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

    pub fn with_transcription(mut self, config: crate::config::TranscriptionConfig) -> Self {
        if !config.enabled {
            return self;
        }
        match super::pipeline::transcription::TranscriptionManager::new(&config) {
            Ok(m) => {
                self.transcription_manager = Some(Arc::new(m));
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

    fn extract_room_id(recipient: &str, fallback_room_id: &str) -> String {
        match recipient.split_once("||") {
            Some((_, room_id)) => room_id.to_string(),
            None => fallback_room_id.to_string(),
        }
    }

    async fn get_joined_room(&self, room_id_str: &str) -> anyhow::Result<matrix_sdk::Room> {
        let client = self.matrix_client().await?;
        let target_room: OwnedRoomId = room_id_str.parse()?;

        let mut room = client.get_room(&target_room);
        if room.is_none() {
            let _ = client.sync_once(SyncSettings::new()).await;
            room = client.get_room(&target_room);
        }

        let room = room.ok_or_else(|| {
            anyhow::anyhow!("Matrix room '{}' not found in joined rooms", room_id_str)
        })?;

        if room.state() != RoomState::Joined {
            anyhow::bail!("Matrix room '{}' is not in joined state", room_id_str);
        }

        Ok(room)
    }

    async fn edit_message(
        &self,
        room_id_str: &str,
        original_event_id: &str,
        new_text: &str,
    ) -> anyhow::Result<()> {
        let room = self.get_joined_room(room_id_str).await?;
        let original_id: OwnedEventId = original_event_id
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid event ID for edit: {}", original_event_id))?;

        let replacement = RoomMessageEventContent::text_markdown(new_text)
            .make_replacement(ReplacementMetadata::new(original_id, None));

        room.send(replacement).await?;
        Ok(())
    }

    fn encode_path_segment(value: &str) -> String {
        fn should_encode(byte: u8) -> bool {
            !matches!(
                byte,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
            )
        }

        let mut encoded = String::with_capacity(value.len());
        for byte in value.bytes() {
            if should_encode(byte) {
                use std::fmt::Write;
                let _ = write!(&mut encoded, "%{byte:02X}");
            } else {
                encoded.push(byte as char);
            }
        }

        encoded
    }

    fn auth_header_value(&self) -> String {
        format!("Bearer {}", self.access_token)
    }

    fn matrix_store_dir(&self) -> Option<PathBuf> {
        self.sen_dir
            .as_ref()
            .map(|dir| dir.join("state").join("matrix"))
    }

    fn device_id_path(&self) -> Option<PathBuf> {
        self.matrix_store_dir().map(|dir| dir.join("device_id"))
    }

    async fn load_or_generate_device_id(&self) -> anyhow::Result<String> {

        if let Some(path) = self.device_id_path() {
            if path.exists() {
                let stored = tokio::fs::read_to_string(&path).await?;
                let stored = stored.trim().to_string();
                if !stored.is_empty() {
                    tracing::info!("Matrix using persisted device_id from {}", path.display());
                    return Ok(stored);
                }
            }
        }

        let device_id = format!("SEN_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        tracing::info!(
            "Matrix auto-generated device_id '{}'. \
             To keep this device stable, it has been saved locally. \
             You can also set channels_config.matrix.device_id in config.toml.",
            device_id
        );

        if let Some(path) = self.device_id_path() {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&path, &device_id).await?;
            tracing::debug!("Matrix persisted device_id to {}", path.display());
        }

        Ok(device_id)
    }

    fn is_user_allowed(&self, sender: &str) -> bool {
        Self::is_sender_allowed(&self.allowed_users, sender)
    }

    fn is_sender_allowed(allowed_users: &[String], sender: &str) -> bool {
        if allowed_users.iter().any(|u| u == "*") {
            return true;
        }

        allowed_users.iter().any(|u| u.eq_ignore_ascii_case(sender))
    }

    fn is_room_allowed_static(allowed_rooms: &[String], room_id: &str) -> bool {
        if allowed_rooms.is_empty() {
            return true;
        }
        allowed_rooms
            .iter()
            .any(|r| r.eq_ignore_ascii_case(room_id))
    }

    fn is_room_allowed(&self, room_id: &str) -> bool {
        Self::is_room_allowed_static(&self.allowed_rooms, room_id)
    }

    fn is_supported_message_type(msgtype: &str) -> bool {
        matches!(msgtype, "m.text" | "m.notice")
    }

    fn has_non_empty_body(body: &str) -> bool {
        !body.trim().is_empty()
    }

    fn room_matches_target(target_room_id: &str, incoming_room_id: &str) -> bool {
        target_room_id == incoming_room_id
    }

    fn cache_event_id(
        event_id: &str,
        recent_order: &mut std::collections::VecDeque<String>,
        recent_lookup: &mut std::collections::HashSet<String>,
    ) -> bool {
        const MAX_RECENT_EVENT_IDS: usize = 2048;

        if recent_lookup.contains(event_id) {
            return true;
        }

        let event_id_owned = event_id.to_string();
        recent_lookup.insert(event_id_owned.clone());
        recent_order.push_back(event_id_owned);

        if recent_order.len() > MAX_RECENT_EVENT_IDS {
            if let Some(evicted) = recent_order.pop_front() {
                recent_lookup.remove(&evicted);
            }
        }

        false
    }

    async fn target_room_id(&self) -> anyhow::Result<String> {
        if self.room_id.starts_with('!') {
            return Ok(self.room_id.clone());
        }

        if let Some(cached) = self.resolved_room_id_cache.read().await.clone() {
            return Ok(cached);
        }

        let resolved = self.resolve_room_id().await?;
        *self.resolved_room_id_cache.write().await = Some(resolved.clone());
        Ok(resolved)
    }

    async fn get_my_identity(&self) -> anyhow::Result<WhoAmIResponse> {
        let url = format!("{}/_matrix/client/v3/account/whoami", self.homeserver);
        let resp = self
            .http_client
            .get(&url)
            .header("Authorization", self.auth_header_value())
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Matrix whoami failed: {err}");
        }

        Ok(resp.json().await?)
    }

    async fn get_my_user_id(&self) -> anyhow::Result<String> {
        Ok(self.get_my_identity().await?.user_id)
    }

    async fn matrix_client(&self) -> anyhow::Result<MatrixSdkClient> {
        let client = self
            .sdk_client
            .get_or_try_init(|| async {
                let identity = self.get_my_identity().await;
                let whoami = match identity {
                    Ok(whoami) => Some(whoami),
                    Err(error) => {
                        if self.session_owner_hint.is_some() && self.session_device_id_hint.is_some()
                        {
                            tracing::warn!(
                                "Matrix whoami failed; falling back to configured session hints for E2EE session restore: {error}. \
                                 See docs/security/matrix-e2ee-guide.md section 4C."
                            );
                            None
                        } else {
                            return Err(error);
                        }
                    }
                };

                let resolved_user_id = if let Some(whoami) = whoami.as_ref() {
                    if let Some(hinted) = self.session_owner_hint.as_ref() {
                        if hinted != &whoami.user_id {
                            tracing::warn!(
                                "Matrix configured user_id '{}' does not match whoami '{}'; using whoami.",
                                crate::security::redact(hinted),
                                crate::security::redact(&whoami.user_id)
                            );
                        }
                    }
                    whoami.user_id.clone()
                } else {
                    self.session_owner_hint.clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "Matrix session restore requires user_id when whoami is unavailable. \
                             Set channels_config.matrix.user_id in config.toml. \
                             See docs/security/matrix-e2ee-guide.md section 2."
                        )
                    })?
                };

                let resolved_device_id = match (whoami.as_ref(), self.session_device_id_hint.as_ref()) {
                    (Some(whoami), Some(hinted)) => {
                        if let Some(whoami_device_id) = whoami.device_id.as_ref() {
                            if whoami_device_id != hinted {
                                tracing::warn!(
                                    "Matrix configured device_id '{}' does not match whoami '{}'; using whoami.",
                                    crate::security::redact(hinted),
                                    crate::security::redact(whoami_device_id)
                                );
                            }
                            whoami_device_id.clone()
                        } else {
                            hinted.clone()
                        }
                    }
                    (Some(whoami), None) => {
                        if let Some(device_id) = whoami.device_id.clone() {
                            device_id
                        } else {
                            tracing::debug!("Matrix whoami did not include device_id, auto-generating");
                            self.load_or_generate_device_id().await?
                        }
                    }
                    (None, Some(hinted)) => hinted.clone(),
                    (None, None) => {
                        tracing::debug!("Matrix no device_id from whoami or config, auto-generating");
                        self.load_or_generate_device_id().await?
                    }
                };

                let mut client_builder = MatrixSdkClient::builder().homeserver_url(&self.homeserver);

                if let Some(store_dir) = self.matrix_store_dir() {
                    tokio::fs::create_dir_all(&store_dir).await.map_err(|error| {
                        anyhow::anyhow!(
                            "Matrix failed to initialize persistent store directory at '{}': {error}",
                            store_dir.display()
                        )
                    })?;
                    client_builder = client_builder.sqlite_store(&store_dir, None);
                }

                let client = client_builder.build().await?;

                let user_id: OwnedUserId = resolved_user_id.parse()?;
                let session = MatrixSession {
                    meta: SessionMeta {
                        user_id,
                        device_id: resolved_device_id.into(),
                    },
                    tokens: SessionTokens {
                        access_token: self.access_token.clone(),
                        refresh_token: None,
                    },
                };

                client.restore_session(session).await?;
                tracing::debug!("Matrix session restored for device");

                if let Some(ref key) = self.recovery_key {
                    match client.encryption().recovery().recover(key).await {
                        Ok(()) => {
                            tracing::info!(
                                "Matrix E2EE recovery successful  -  room keys and cross-signing secrets restored from server backup."
                            );
                        }
                        Err(error) => {
                            tracing::warn!(
                                "Matrix E2EE recovery failed: {error}. \
                                 The recovery key may be incorrect, or server-side key backup may not be configured. \
                                 The bot will still work in unencrypted rooms. \
                                 See docs/security/matrix-e2ee-guide.md section 4I."
                            );
                        }
                    }
                }

                Ok::<MatrixSdkClient, anyhow::Error>(client)
            })
            .await?;

        Ok(client.clone())
    }

    async fn resolve_room_id(&self) -> anyhow::Result<String> {
        let configured = self.room_id.trim();

        if configured.starts_with('!') {
            return Ok(configured.to_string());
        }

        if configured.starts_with('#') {
            let encoded_alias = Self::encode_path_segment(configured);
            let url = format!(
                "{}/_matrix/client/v3/directory/room/{}",
                self.homeserver, encoded_alias
            );

            let resp = self
                .http_client
                .get(&url)
                .header("Authorization", self.auth_header_value())
                .send()
                .await?;

            if !resp.status().is_success() {
                let err = resp.text().await.unwrap_or_default();
                anyhow::bail!("Matrix room alias resolution failed for '{configured}': {err}");
            }

            let resolved: RoomAliasResponse = resp.json().await?;
            return Ok(resolved.room_id);
        }

        anyhow::bail!(
            "Matrix room reference must start with '!' (room ID) or '#' (room alias), got: {configured}"
        )
    }

    async fn ensure_room_accessible(&self, room_id: &str) -> anyhow::Result<()> {
        let encoded_room = Self::encode_path_segment(room_id);
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/joined_members",
            self.homeserver, encoded_room
        );

        let resp = self
            .http_client
            .get(&url)
            .header("Authorization", self.auth_header_value())
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Matrix room access check failed for '{room_id}': {err}");
        }

        Ok(())
    }

    async fn room_is_encrypted(&self, room_id: &str) -> anyhow::Result<bool> {
        let encoded_room = Self::encode_path_segment(room_id);
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/state/m.room.encryption",
            self.homeserver, encoded_room
        );

        let resp = self
            .http_client
            .get(&url)
            .header("Authorization", self.auth_header_value())
            .send()
            .await?;

        if resp.status().is_success() {
            return Ok(true);
        }

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }

        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Matrix room encryption check failed for '{room_id}': {err}");
    }

    async fn ensure_room_supported(&self, room_id: &str) -> anyhow::Result<()> {
        self.ensure_room_accessible(room_id).await?;

        if self.room_is_encrypted(room_id).await? {
            tracing::info!(
                "Matrix room {} is encrypted; E2EE decryption is enabled via matrix-sdk.",
                room_id
            );
        }

        Ok(())
    }

    fn sync_filter_for_room(room_id: &str, timeline_limit: usize) -> String {
        let timeline_limit = timeline_limit.max(1);
        serde_json::json!({
            "room": {
                "rooms": [room_id],
                "timeline": {
                    "limit": timeline_limit
                }
            }
        })
        .to_string()
    }

    async fn log_e2ee_diagnostics(&self, client: &MatrixSdkClient) {
        match client.encryption().get_own_device().await {
            Ok(Some(device)) => {
                if device.is_verified() {
                    tracing::info!(
                        "Matrix device '{}' is verified for E2EE.",
                        device.device_id()
                    );
                } else {
                    tracing::warn!(
                        "Matrix device '{}' is not verified. Other clients will label bot messages as unverified. \
                         Verify this device from a trusted session and keep device_id stable across restarts. \
                         See docs/security/matrix-e2ee-guide.md section 4D.",
                        device.device_id()
                    );
                }
            }
            Ok(None) => {
                tracing::warn!(
                    "Matrix own-device metadata is unavailable; verify/signing status cannot be determined. \
                     See docs/security/matrix-e2ee-guide.md section 4D."
                );
            }
            Err(error) => {
                tracing::warn!("Matrix own-device verification check failed: {error}");
            }
        }

        if client.encryption().backups().are_enabled().await {
            tracing::info!("Matrix room-key backup is enabled for this device.");
        } else {
            let _ = client.encryption().backups().disable().await;
            if self.recovery_key.is_some() {
                tracing::info!(
                    "Matrix room-key backup is not active on this device, but a recovery key is configured. \
                     Room keys will be restored from server backup on startup."
                );
            } else {
                tracing::warn!(
                    "Matrix room-key backup is not enabled for this device. \
                     To automatically restore room keys after a device reset, set recovery_key in your Matrix config. \
                     See docs/security/matrix-e2ee-guide.md section 4I."
                );
            }
        }
    }
}

#[async_trait]
impl Channel for MatrixChannel {
    fn name(&self) -> &str {
        "matrix"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        if self.otk_conflict_detected.load(Ordering::Relaxed) {
            tracing::debug!("Matrix OTK conflict flag is set, refusing send");
            anyhow::bail!("Matrix channel unavailable: E2EE one-time key conflict detected");
        }
        let client = self.matrix_client().await?;
        let target_room_id = match message.recipient.split_once("||") {
            Some((_, room_id)) => room_id.to_string(),
            None => self.target_room_id().await?,
        };
        let target_room: OwnedRoomId = target_room_id.parse()?;

        let mut room = client.get_room(&target_room);
        if room.is_none() {
            let _ = client.sync_once(SyncSettings::new()).await;
            room = client.get_room(&target_room);
        }

        let Some(room) = room else {
            anyhow::bail!("Matrix room '{}' not found in joined rooms", target_room_id);
        };

        if room.state() != RoomState::Joined {
            anyhow::bail!("Matrix room '{}' is not in joined state", target_room_id);
        }

        if let Err(error) = room.typing_notice(false).await {
            tracing::warn!("Matrix failed to stop typing notification: {error}");
        }

        let mut content = RoomMessageEventContent::text_markdown(&message.content);

        if let Some(ref thread_ts) = message.thread_ts {
            if let Ok(thread_root) = thread_ts.parse::<OwnedEventId>() {
                content.relates_to = Some(Relation::Thread(Thread::plain(
                    thread_root.clone(),
                    thread_root,
                )));
            }
        }

        room.send(content).await?;

        if self.voice_mode.load(Ordering::Relaxed) {
            self.voice_mode.store(false, Ordering::Relaxed);
            tracing::info!("Voice mode active, generating TTS reply");
            let voice_work = std::path::PathBuf::from("/tmp/sen-voice");
            let _ = tokio::fs::create_dir_all(&voice_work).await;
            let mp3_path = voice_work.join("reply.mp3");

            let tts_text = message
                .content
                .replace("**", "")
                .replace(['*', '`'], "")
                .replace("# ", "");

            let tts_ok = crate::util::hidden_async_command("edge-tts")
                .arg("--text")
                .arg(&tts_text)
                .arg("--write-media")
                .arg(&mp3_path)
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false);

            if tts_ok && mp3_path.exists() {
                if let Ok(audio_data) = tokio::fs::read(&mp3_path).await {
                    let upload_url = format!(
                        "{}/_matrix/media/v3/upload?filename=voice-reply.mp3",
                        self.homeserver
                    );
                    if let Ok(resp) = self
                        .http_client
                        .post(&upload_url)
                        .header("Authorization", self.auth_header_value())
                        .header("Content-Type", "audio/mpeg")
                        .body(audio_data)
                        .send()
                        .await
                    {
                        if resp.status().is_success() {
                            if let Ok(body) = resp.json::<serde_json::Value>().await {
                                if let Some(content_uri) = body["content_uri"].as_str() {
                                    let encoded_room = Self::encode_path_segment(&target_room_id);
                                    let txn_id = format!(
                                        "voice_{}",
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis()
                                    );
                                    let audio_msg = serde_json::json!({
                                        "msgtype": "m.audio",
                                        "body": "Voice reply",
                                        "url": content_uri,
                                        "info": { "mimetype": "audio/mpeg" }
                                    });
                                    let send_url = format!(
                                        "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
                                        self.homeserver, encoded_room, txn_id
                                    );
                                    let _ = self
                                        .http_client
                                        .put(&send_url)
                                        .header("Authorization", self.auth_header_value())
                                        .json(&audio_msg)
                                        .send()
                                        .await;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        if self.otk_conflict_detected.load(Ordering::Relaxed) {
            tracing::debug!("Matrix OTK conflict flag is set, refusing listen");
            anyhow::bail!("Matrix channel unavailable: E2EE one-time key conflict detected");
        }
        let target_room_id = self.target_room_id().await?;
        self.ensure_room_supported(&target_room_id).await?;

        let target_room: OwnedRoomId = target_room_id.parse()?;
        let my_user_id: OwnedUserId = match self.get_my_user_id().await {
            Ok(user_id) => user_id.parse()?,
            Err(error) => {
                if let Some(hinted) = self.session_owner_hint.as_ref() {
                    tracing::warn!(
                        "Matrix whoami failed while resolving listener user_id; using configured user_id hint: {error}"
                    );
                    hinted.parse()?
                } else {
                    return Err(error);
                }
            }
        };
        let client = self.matrix_client().await?;

        self.log_e2ee_diagnostics(&client).await;

        let _ = client.sync_once(SyncSettings::new()).await;

        if self.allowed_rooms.is_empty() {
            tracing::info!(
                "Matrix channel listening on room {} (configured as {})...",
                target_room_id,
                self.room_id
            );
        } else {
            tracing::info!(
                "Matrix channel listening on {} allowed room(s) (primary: {})...",
                self.allowed_rooms.len(),
                self.room_id
            );
        }

        let recent_event_cache = Arc::new(Mutex::new((
            std::collections::VecDeque::new(),
            std::collections::HashSet::new(),
        )));

        let tx_handler = tx.clone();
        let target_room_for_handler = target_room.clone();
        let my_user_id_for_handler = my_user_id.clone();
        let allowed_users_for_handler = self.allowed_users.clone();
        let allowed_rooms_for_handler = self.allowed_rooms.clone();
        let dedupe_for_handler = Arc::clone(&recent_event_cache);
        let homeserver_for_handler = self.homeserver.clone();
        let access_token_for_handler = self.access_token.clone();
        let voice_mode_for_handler = Arc::clone(&self.voice_mode);
        let transcription_mgr_for_handler = self.transcription_manager.clone();

        client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
            let tx = tx_handler.clone();
            let target_room = target_room_for_handler.clone();
            let my_user_id = my_user_id_for_handler.clone();
            let allowed_users = allowed_users_for_handler.clone();
            let allowed_rooms = allowed_rooms_for_handler.clone();
            let dedupe = Arc::clone(&dedupe_for_handler);
            let homeserver = homeserver_for_handler.clone();
            let access_token = access_token_for_handler.clone();
            let voice_mode = Arc::clone(&voice_mode_for_handler);
            let transcription_mgr = transcription_mgr_for_handler.clone();

            async move {

                if allowed_rooms.is_empty() {
                    if !MatrixChannel::room_matches_target(
                        target_room.as_str(),
                        room.room_id().as_str(),
                    ) {
                        tracing::debug!(
                            "Matrix: ignoring message from room {} (not the configured room_id)",
                            room.room_id()
                        );
                        return;
                    }
                } else if !MatrixChannel::is_room_allowed_static(&allowed_rooms, room.room_id().as_ref()) {
                    tracing::debug!(
                        "Matrix: ignoring message from room {} (not in allowed_rooms)",
                        room.room_id()
                    );
                    return;
                }

                tracing::debug!(
                    "Matrix: received message in room {} from {}",
                    room.room_id(),
                    event.sender
                );

                if event.sender == my_user_id {
                    tracing::debug!("Matrix: ignoring own message");
                    return;
                }

                let sender = event.sender.to_string();
                if !MatrixChannel::is_sender_allowed(&allowed_users, &sender) {
                    tracing::debug!("Matrix: ignoring message from non-allowed user {sender}");
                    return;
                }

                let media_info = |source: &MediaSource, name: &str| -> Option<(String, String)> {
                    match source {
                        MediaSource::Plain(mxc) => {
                            let rest = mxc.as_str().strip_prefix("mxc://")?;
                            let url =
                                format!("{}/_matrix/client/v1/media/download/{}", homeserver, rest);
                            Some((url, name.to_string()))
                        }
                        MediaSource::Encrypted(_) => None,
                    }
                };

                let (body, media_download) = match &event.content.msgtype {
                    MessageType::Text(content) => (content.body.clone(), None),
                    MessageType::Notice(content) => (content.body.clone(), None),
                    MessageType::Image(content) => {
                        let dl = media_info(&content.source, &content.body);
                        (format!("[IMAGE:{}]", content.body), dl)
                    }
                    MessageType::File(content) => {
                        let dl = media_info(&content.source, &content.body);
                        (format!("[file: {}]", content.body), dl)
                    }
                    MessageType::Audio(content) => {
                        let dl = media_info(&content.source, &content.body);
                        (format!("[audio: {}]", content.body), dl)
                    }
                    MessageType::Video(content) => {
                        let dl = media_info(&content.source, &content.body);
                        (format!("[video: {}]", content.body), dl)
                    }
                    _ => return,
                };

                let body = if let Some((url, filename)) = media_download {
                    let workspace = std::path::PathBuf::from(
                        shellexpand::tilde(
                            &std::env::var("SEN_WORKSPACE")
                                .unwrap_or_else(|_| "/tmp/sen-uploads".to_string()),
                        )
                        .as_ref(),
                    );
                    let _ = tokio::fs::create_dir_all(&workspace).await;
                    let dest = workspace.join(&filename);
                    let client = reqwest::Client::new();
                    match client
                        .get(&url)
                        .header("Authorization", format!("Bearer {}", access_token))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                            Ok(bytes) => match tokio::fs::write(&dest, &bytes).await {
                                Ok(()) => {
                                    if body.starts_with("[IMAGE:") {
                                        format!("[IMAGE:{}]", dest.display())
                                    } else {
                                        format!("{}  -  saved to {}", body, dest.display())
                                    }
                                }
                                Err(_) => format!("{}  -  failed to write to disk", body),
                            },
                            Err(_) => format!("{}  -  download failed", body),
                        },
                        _ => format!("{}  -  download failed (auth error?)", body),
                    }
                } else {
                    body
                };

                let body = if body.starts_with("[audio:") {
                    if let (Some(path_start), Some(ref manager)) = (body.find("saved to "), &transcription_mgr) {
                        let audio_path = body[path_start + 9..].to_string();
                        let file_name = audio_path
                            .rsplit('/')
                            .next()
                            .unwrap_or("audio.ogg")
                            .to_string();
                        match tokio::fs::read(&audio_path).await {
                            Ok(audio_data) => {
                                match manager.transcribe(&audio_data, &file_name).await {
                                    Ok(text) => {
                                        let trimmed = text.trim();
                                        if trimmed.is_empty() {
                                            tracing::info!("Matrix: voice transcription returned empty text, skipping");
                                            body
                                        } else {
                                            voice_mode.store(true, Ordering::Relaxed);
                                            format!("[Voice message]: {}", trimmed)
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Matrix: voice transcription failed: {e}");
                                        body
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Matrix: failed to read audio file {}: {e}", audio_path);
                                body
                            }
                        }
                    } else {
                        body
                    }
                } else {
                    body
                };

                if !MatrixChannel::has_non_empty_body(&body) {
                    return;
                }

                let event_id = event.event_id.to_string();
                {
                    let mut guard = dedupe.lock().await;
                    let (recent_order, recent_lookup) = &mut *guard;
                    if MatrixChannel::cache_event_id(&event_id, recent_order, recent_lookup) {
                        return;
                    }
                }

                if let Err(error) = room
                    .send_single_receipt(
                        create_receipt::v3::ReceiptType::Read,
                        ReceiptThread::Unthreaded,
                        event.event_id.clone(),
                    )
                    .await
                {
                    tracing::warn!("Matrix failed to send read receipt: {error}");
                }

                if let Err(error) = room.typing_notice(true).await {
                    tracing::warn!("Matrix failed to start typing notification: {error}");
                }

                let thread_ts = match &event.content.relates_to {
                    Some(Relation::Thread(thread)) => Some(thread.event_id.to_string()),
                    _ => None,
                };
                let msg = ChannelMessage {
                    id: event_id,
                    sender: sender.clone(),
                    reply_target: format!("{}||{}", sender, room.room_id()),
                    content: body,
                    channel: "matrix".to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    thread_ts: thread_ts.clone(),
                    interruption_scope_id: thread_ts,
                    attachments: vec![],
                };

                let _ = tx.send(msg).await;
            }
        });

        let allowed_rooms_for_invite = self.allowed_rooms.clone();
        client.add_event_handler(move |event: StrippedRoomMemberEvent, room: Room| {
            let allowed_rooms = allowed_rooms_for_invite.clone();
            async move {

                if event.content.membership
                    != matrix_sdk::ruma::events::room::member::MembershipState::Invite
                {
                    return;
                }

                let room_id_str = room.room_id().to_string();

                if MatrixChannel::is_room_allowed_static(&allowed_rooms, &room_id_str) {

                    tracing::info!(
                        "Matrix: auto-accepting invite for allowed room {}",
                        room_id_str
                    );
                    if let Err(error) = room.join().await {
                        tracing::warn!("Matrix: failed to auto-join room {}: {error}", room_id_str);
                    }
                } else {

                    tracing::info!(
                        "Matrix: auto-rejecting invite for room {} (not in allowed_rooms)",
                        room_id_str
                    );
                    if let Err(error) = room.leave().await {
                        tracing::warn!(
                            "Matrix: failed to reject invite for room {}: {error}",
                            room_id_str
                        );
                    }
                }
            }
        });

        let sync_settings = SyncSettings::new().timeout(std::time::Duration::from_secs(30));
        let otk_conflict_detected = Arc::clone(&self.otk_conflict_detected);
        client
            .sync_with_result_callback(sync_settings, |sync_result| {
                let tx = tx.clone();
                let otk_conflict_detected = Arc::clone(&otk_conflict_detected);
                async move {
                    if tx.is_closed() {
                        return Ok::<LoopCtrl, matrix_sdk::Error>(LoopCtrl::Break);
                    }

                    if let Err(error) = sync_result {
                        let raw = error.to_string();
                        let safe_error = MatrixChannel::sanitize_error_for_log(&error);

                        if MatrixChannel::is_otk_conflict_message(&raw) {
                            otk_conflict_detected.store(true, Ordering::SeqCst);
                            tracing::error!(
                                "Matrix one-time key upload conflict detected; \
                                 stopping sync to avoid infinite retry loop."
                            );
                            return Ok::<LoopCtrl, matrix_sdk::Error>(LoopCtrl::Break);
                        }

                        tracing::debug!(error = %safe_error, "Matrix sync error classified as transient, retrying");
                        tracing::warn!("Matrix sync error: {safe_error}, retrying...");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    } else {
                        tracing::debug!("Matrix sync cycle completed");
                    }

                    Ok::<LoopCtrl, matrix_sdk::Error>(LoopCtrl::Continue)
                }
            })
            .await?;

        if self.otk_conflict_detected.load(Ordering::Relaxed) {
            let mut msg = String::from(
                "Matrix E2EE one-time key conflict detected. \
                 Deregister the stale device, delete the local crypto store, and restart. \
                 See docs/security/matrix-e2ee-guide.md section 4H.",
            );
            if let Some(store_dir) = self.matrix_store_dir() {
                use std::fmt::Write;
                let _ = write!(msg, " Store path: {}", store_dir.display());
            }
            anyhow::bail!("{msg}");
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        if self.otk_conflict_detected.load(Ordering::Relaxed) {
            tracing::debug!("Matrix health check: unhealthy (OTK conflict)");
            return false;
        }

        let Ok(room_id) = self.target_room_id().await else {
            return false;
        };

        if self.ensure_room_supported(&room_id).await.is_err() {
            return false;
        }

        let healthy = self.matrix_client().await.is_ok();
        tracing::debug!(healthy, "Matrix health check result");
        healthy
    }

    async fn add_reaction(
        &self,
        _channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let client = self.matrix_client().await?;
        let target_room_id = self.target_room_id().await?;
        let target_room: OwnedRoomId = target_room_id.parse()?;

        let room = client
            .get_room(&target_room)
            .ok_or_else(|| anyhow::anyhow!("Matrix room not found for reaction"))?;

        let event_id: OwnedEventId = message_id
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid event ID for reaction: {}", message_id))?;

        let reaction = ReactionEventContent::new(Annotation::new(event_id, emoji.to_string()));
        let response = room.send(reaction).await?;

        let key = format!("{}:{}", message_id, emoji);
        self.reaction_events
            .write()
            .await
            .insert(key, response.event_id.to_string());

        Ok(())
    }

    async fn remove_reaction(
        &self,
        _channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let key = format!("{}:{}", message_id, emoji);
        let reaction_event_id = self.reaction_events.write().await.remove(&key);

        if let Some(reaction_event_id) = reaction_event_id {
            let client = self.matrix_client().await?;
            let target_room_id = self.target_room_id().await?;
            let target_room: OwnedRoomId = target_room_id.parse()?;

            let room = client
                .get_room(&target_room)
                .ok_or_else(|| anyhow::anyhow!("Matrix room not found for reaction removal"))?;

            let event_id: OwnedEventId = reaction_event_id
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid reaction event ID: {}", reaction_event_id))?;

            room.redact(&event_id, None, None).await?;
        }

        Ok(())
    }

    async fn pin_message(&self, _channel_id: &str, message_id: &str) -> anyhow::Result<()> {
        let room_id = self.target_room_id().await?;
        let encoded_room = Self::encode_path_segment(&room_id);

        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/state/m.room.pinned_events",
            self.homeserver, encoded_room
        );
        let resp = self
            .http_client
            .get(&url)
            .header("Authorization", self.auth_header_value())
            .send()
            .await?;

        let mut pinned: Vec<String> = if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await?;
            body.get("pinned")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let msg_id = message_id.to_string();
        if pinned.contains(&msg_id) {
            return Ok(());
        }
        pinned.push(msg_id);

        let put_url = format!(
            "{}/_matrix/client/v3/rooms/{}/state/m.room.pinned_events",
            self.homeserver, encoded_room
        );
        let body = serde_json::json!({ "pinned": pinned });
        let resp = self
            .http_client
            .put(&put_url)
            .header("Authorization", self.auth_header_value())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Matrix pin_message failed: {err}");
        }

        Ok(())
    }

    async fn unpin_message(&self, _channel_id: &str, message_id: &str) -> anyhow::Result<()> {
        let room_id = self.target_room_id().await?;
        let encoded_room = Self::encode_path_segment(&room_id);

        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/state/m.room.pinned_events",
            self.homeserver, encoded_room
        );
        let resp = self
            .http_client
            .get(&url)
            .header("Authorization", self.auth_header_value())
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(());
        }

        let body: serde_json::Value = resp.json().await?;
        let mut pinned: Vec<String> = body
            .get("pinned")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let msg_id = message_id.to_string();
        let original_len = pinned.len();
        pinned.retain(|id| id != &msg_id);

        if pinned.len() == original_len {
            return Ok(());
        }

        let put_url = format!(
            "{}/_matrix/client/v3/rooms/{}/state/m.room.pinned_events",
            self.homeserver, encoded_room
        );
        let body = serde_json::json!({ "pinned": pinned });
        let resp = self
            .http_client
            .put(&put_url)
            .header("Authorization", self.auth_header_value())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Matrix unpin_message failed: {err}");
        }

        Ok(())
    }

    async fn redact_message(
        &self,
        _channel_id: &str,
        message_id: &str,
        reason: Option<String>,
    ) -> anyhow::Result<()> {
        let client = self
            .sdk_client
            .get()
            .ok_or_else(|| anyhow::anyhow!("Matrix SDK client not initialized"))?;

        let target_room_id = self.target_room_id().await?;
        let target_room: OwnedRoomId = target_room_id.parse()?;
        let room = client
            .get_room(&target_room)
            .ok_or_else(|| anyhow::anyhow!("Matrix room not found for message redaction"))?;

        let event_id: OwnedEventId = message_id
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid event ID: {}", message_id))?;

        room.redact(&event_id, reason.as_deref(), None).await?;
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        self.stream_mode != crate::config::StreamMode::Off
    }

    async fn send_draft(&self, message: &SendMessage) -> anyhow::Result<Option<String>> {
        use crate::config::StreamMode;
        match self.stream_mode {
            StreamMode::Off => Ok(None),
            StreamMode::Partial => {

                let room_id = Self::extract_room_id(&message.recipient, &self.room_id);
                let room = self.get_joined_room(&room_id).await?;

                let initial_text = if message.content.is_empty() {
                    "..."
                } else {
                    &message.content
                };

                let mut content = RoomMessageEventContent::text_markdown(initial_text);

                if let Some(ref thread_ts) = message.thread_ts {
                    if let Ok(thread_root) = thread_ts.parse::<OwnedEventId>() {
                        content.relates_to = Some(Relation::Thread(Thread::plain(
                            thread_root.clone(),
                            thread_root,
                        )));
                    }
                }

                let response = room.send(content).await?;
                let event_id = response.event_id.to_string();

                self.last_draft_edit
                    .lock()
                    .await
                    .insert(room_id, std::time::Instant::now());

                Ok(Some(event_id))
            }
            StreamMode::MultiMessage => {

                let room_id = Self::extract_room_id(&message.recipient, &self.room_id);
                self.multi_message_sent_len.lock().await.clear();
                self.multi_message_thread_ts
                    .lock()
                    .await
                    .insert(room_id, message.thread_ts.clone());
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
        let room_id = Self::extract_room_id(recipient, &self.room_id);

        match self.stream_mode {
            StreamMode::Off => Ok(()),
            StreamMode::Partial => {

                {
                    let last_edits = self.last_draft_edit.lock().await;
                    if let Some(last_time) = last_edits.get(&room_id) {
                        let elapsed =
                            u64::try_from(last_time.elapsed().as_millis()).unwrap_or(u64::MAX);
                        if elapsed < self.draft_update_interval_ms {
                            return Ok(());
                        }
                    }
                }

                if let Err(e) = self.edit_message(&room_id, message_id, text).await {
                    tracing::debug!("Matrix draft update edit failed: {e}");
                    return Ok(());
                }

                self.last_draft_edit
                    .lock()
                    .await
                    .insert(room_id, std::time::Instant::now());

                Ok(())
            }
            StreamMode::MultiMessage => {

                let thread_ts = self
                    .multi_message_thread_ts
                    .lock()
                    .await
                    .get(&room_id)
                    .cloned()
                    .flatten();
                let mut sent_map = self.multi_message_sent_len.lock().await;
                let sent_so_far = sent_map.get(&room_id).copied().unwrap_or(0);

                if text.len() < sent_so_far {
                    sent_map.insert(room_id.clone(), 0);
                    return Ok(());
                }
                if text.len() == sent_so_far {
                    return Ok(());
                }

                let new_text = &text[sent_so_far..];

                let mut scan_pos = 0;
                let mut in_fence = false;
                let bytes = new_text.as_bytes();

                while scan_pos < bytes.len() {
                    let ch = bytes[scan_pos];

                    if ch == b'`'
                        && scan_pos + 2 < bytes.len()
                        && bytes[scan_pos + 1] == b'`'
                        && bytes[scan_pos + 2] == b'`'
                        && (scan_pos == 0
                            || bytes[scan_pos - 1] == b'\n'
                            || (sent_so_far + scan_pos == 0))
                    {
                        in_fence = !in_fence;
                    }

                    if !in_fence
                        && ch == b'\n'
                        && scan_pos + 1 < bytes.len()
                        && bytes[scan_pos + 1] == b'\n'
                    {
                        let paragraph = new_text[..scan_pos].trim().to_string();
                        if !paragraph.is_empty() {
                            let msg = SendMessage::new(&paragraph, recipient)
                                .in_thread(thread_ts.clone());
                            if let Err(e) = self.send(&msg).await {
                                tracing::debug!("Multi-message paragraph send failed: {e}");
                            }
                            if self.multi_message_delay_ms > 0 {
                                tokio::time::sleep(std::time::Duration::from_millis(
                                    self.multi_message_delay_ms,
                                ))
                                .await;
                            }
                        }

                        let consumed = scan_pos + 2;
                        *sent_map.entry(room_id.clone()).or_insert(0) += consumed;

                        let remaining = &new_text[consumed..];
                        if !remaining.is_empty() {
                            drop(sent_map);
                            return self.update_draft(recipient, message_id, text).await;
                        }
                        return Ok(());
                    }

                    scan_pos += 1;
                }

                Ok(())
            }
        }
    }

    async fn update_draft_progress(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {

        if self.stream_mode == crate::config::StreamMode::Partial {
            self.update_draft(recipient, message_id, text).await
        } else {
            Ok(())
        }
    }

    async fn finalize_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        use crate::config::StreamMode;
        let room_id = Self::extract_room_id(recipient, &self.room_id);

        match self.stream_mode {
            StreamMode::Off => Ok(()),
            StreamMode::Partial => {

                self.last_draft_edit.lock().await.remove(&room_id);
                self.edit_message(&room_id, message_id, text).await
            }
            StreamMode::MultiMessage => {

                let mut sent_map = self.multi_message_sent_len.lock().await;
                let sent_so_far = sent_map.get(&room_id).copied().unwrap_or(0);

                if text.len() > sent_so_far {
                    let remaining = text[sent_so_far..].trim().to_string();
                    if !remaining.is_empty() {
                        let thread_ts = self
                            .multi_message_thread_ts
                            .lock()
                            .await
                            .get(&room_id)
                            .cloned()
                            .flatten();
                        let msg = SendMessage::new(&remaining, recipient).in_thread(thread_ts);
                        if let Err(e) = self.send(&msg).await {
                            tracing::debug!("Multi-message final flush failed: {e}");
                        }
                    }
                }

                sent_map.remove(&room_id);
                self.multi_message_thread_ts.lock().await.remove(&room_id);
                Ok(())
            }
        }
    }

    async fn cancel_draft(&self, recipient: &str, message_id: &str) -> anyhow::Result<()> {
        use crate::config::StreamMode;
        let room_id = Self::extract_room_id(recipient, &self.room_id);

        match self.stream_mode {
            StreamMode::Off => Ok(()),
            StreamMode::Partial => {

                self.last_draft_edit.lock().await.remove(&room_id);
                self.redact_message(&room_id, message_id, None).await
            }
            StreamMode::MultiMessage => {

                self.multi_message_sent_len.lock().await.remove(&room_id);
                self.multi_message_thread_ts.lock().await.remove(&room_id);
                Ok(())
            }
        }
    }
}
