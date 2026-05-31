// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Channel, ChannelMessage, SendMessage};
use super::storage::RusqliteStore;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::select;
use wa_rs_proto::whatsapp::device_props::PlatformType;

#[cfg(feature = "whatsapp-web")]
pub struct WhatsAppWebChannel {

    session_path: String,

    pair_phone: Option<String>,

    pair_code: Option<String>,

    allowed_numbers: Vec<String>,

    mode: crate::config::WhatsAppWebMode,

    dm_policy: crate::config::WhatsAppChatPolicy,

    group_policy: crate::config::WhatsAppChatPolicy,

    self_chat_mode: bool,

    bot_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,

    client: Arc<Mutex<Option<Arc<wa_rs::Client>>>>,

    tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>>,

    transcription: Option<crate::config::TranscriptionConfig>,
    transcription_manager: Option<std::sync::Arc<super::super::pipeline::transcription::TranscriptionManager>>,

    tts_config: Option<crate::config::TtsConfig>,

    pending_voice:
        Arc<std::sync::Mutex<std::collections::HashMap<String, (String, std::time::Instant)>>>,

    voice_chats: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,

    dm_mention_patterns: Arc<Vec<regex::Regex>>,

    group_mention_patterns: Arc<Vec<regex::Regex>>,
}

impl WhatsAppWebChannel {

    #[cfg(feature = "whatsapp-web")]
    pub fn new(
        session_path: String,
        pair_phone: Option<String>,
        pair_code: Option<String>,
        allowed_numbers: Vec<String>,
        mode: crate::config::WhatsAppWebMode,
        dm_policy: crate::config::WhatsAppChatPolicy,
        group_policy: crate::config::WhatsAppChatPolicy,
        self_chat_mode: bool,
    ) -> Self {
        Self {
            session_path,
            pair_phone,
            pair_code,
            allowed_numbers,
            mode,
            dm_policy,
            group_policy,
            self_chat_mode,
            bot_handle: Arc::new(Mutex::new(None)),
            client: Arc::new(Mutex::new(None)),
            tx: Arc::new(Mutex::new(None)),
            transcription: None,
            transcription_manager: None,
            tts_config: None,
            pending_voice: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            voice_chats: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            dm_mention_patterns: Arc::new(Vec::new()),
            group_mention_patterns: Arc::new(Vec::new()),
        }
    }

    #[cfg(feature = "whatsapp-web")]
    pub fn with_transcription(mut self, config: crate::config::TranscriptionConfig) -> Self {
        if !config.enabled {
            return self;
        }
        match super::super::pipeline::transcription::TranscriptionManager::new(&config) {
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

    #[cfg(feature = "whatsapp-web")]
    pub fn with_tts(mut self, config: crate::config::TtsConfig) -> Self {
        if config.enabled {
            self.tts_config = Some(config);
        }
        self
    }

    #[cfg(feature = "whatsapp-web")]
    pub fn with_dm_mention_patterns(mut self, patterns: Vec<String>) -> Self {
        self.dm_mention_patterns = Arc::new(
            super::WhatsAppChannel::compile_mention_patterns(&patterns),
        );
        self
    }

    #[cfg(feature = "whatsapp-web")]
    pub fn with_group_mention_patterns(mut self, patterns: Vec<String>) -> Self {
        self.group_mention_patterns = Arc::new(
            super::WhatsAppChannel::compile_mention_patterns(&patterns),
        );
        self
    }

    #[cfg(feature = "whatsapp-web")]
    fn is_number_allowed(&self, phone: &str) -> bool {
        Self::is_number_allowed_for_list(&self.allowed_numbers, phone)
    }

    #[cfg(feature = "whatsapp-web")]
    fn is_number_allowed_for_list(allowed_numbers: &[String], phone: &str) -> bool {
        if allowed_numbers.iter().any(|entry| entry.trim() == "*") {
            return true;
        }

        let Some(phone_norm) = Self::normalize_phone_token(phone) else {
            return false;
        };

        allowed_numbers.iter().any(|entry| {
            Self::normalize_phone_token(entry)
                .as_deref()
                .is_some_and(|allowed_norm| allowed_norm == phone_norm)
        })
    }

    #[cfg(feature = "whatsapp-web")]
    fn normalize_phone_token(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        let user_part = trimmed
            .split_once('@')
            .map(|(user, _)| user)
            .unwrap_or(trimmed)
            .trim();

        let digits: String = user_part.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            None
        } else {
            Some(format!("+{digits}"))
        }
    }

    #[cfg(feature = "whatsapp-web")]
    fn sender_phone_candidates(
        sender: &wa_rs_binary::jid::Jid,
        sender_alt: Option<&wa_rs_binary::jid::Jid>,
        mapped_phone: Option<&str>,
    ) -> Vec<String> {
        let mut candidates = Vec::new();

        let mut add_candidate = |candidate: Option<String>| {
            if let Some(candidate) = candidate {
                if !candidates.iter().any(|existing| existing == &candidate) {
                    candidates.push(candidate);
                }
            }
        };

        add_candidate(Self::normalize_phone_token(&sender.to_string()));
        if let Some(alt) = sender_alt {
            add_candidate(Self::normalize_phone_token(&alt.to_string()));
        }
        if let Some(mapped_phone) = mapped_phone {
            add_candidate(Self::normalize_phone_token(mapped_phone));
        }

        candidates
    }

    #[cfg(feature = "whatsapp-web")]
    fn normalize_phone(&self, phone: &str) -> String {
        if let Some(normalized) = Self::normalize_phone_token(phone) {
            return normalized;
        }

        let trimmed = phone.trim();
        let user_part = trimmed
            .split_once('@')
            .map(|(user, _)| user)
            .unwrap_or(trimmed);
        let normalized_user = user_part.trim_start_matches('+');
        format!("+{normalized_user}")
    }

    #[cfg(feature = "whatsapp-web")]
    fn is_jid(recipient: &str) -> bool {
        recipient.trim().contains('@')
    }

    #[cfg(feature = "whatsapp-web")]
    fn render_pairing_qr(code: &str) -> Result<String> {
        let payload = code.trim();
        if payload.is_empty() {
            anyhow::bail!("QR payload is empty");
        }

        let qr = qrcode::QrCode::new(payload.as_bytes())
            .map_err(|err| anyhow!("Failed to encode WhatsApp Web QR payload: {err}"))?;

        Ok(qr
            .render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build())
    }

    #[cfg(feature = "whatsapp-web")]
    fn recipient_to_jid(&self, recipient: &str) -> Result<wa_rs_binary::jid::Jid> {
        let trimmed = recipient.trim();
        if trimmed.is_empty() {
            anyhow::bail!("Recipient cannot be empty");
        }

        if trimmed.contains('@') {
            return trimmed
                .parse::<wa_rs_binary::jid::Jid>()
                .map_err(|e| anyhow!("Invalid WhatsApp JID `{trimmed}`: {e}"));
        }

        let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            anyhow::bail!("Recipient `{trimmed}` does not contain a valid phone number");
        }

        Ok(wa_rs_binary::jid::Jid::pn(digits))
    }

    const MAX_RETRIES: u32 = 10;
    const BASE_DELAY_SECS: u64 = 3;
    const MAX_DELAY_SECS: u64 = 300;

    fn compute_retry_delay(attempt: u32) -> u64 {
        std::cmp::min(
            Self::BASE_DELAY_SECS.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1))),
            Self::MAX_DELAY_SECS,
        )
    }

    fn should_purge_session(session_revoked: &std::sync::atomic::AtomicBool) -> bool {
        session_revoked.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn record_retry(retry_count: &std::sync::atomic::AtomicU32) -> (u32, bool) {
        let attempts = retry_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        (attempts, attempts > Self::MAX_RETRIES)
    }

    fn reset_retry(retry_count: &std::sync::atomic::AtomicU32) {
        retry_count.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    fn session_file_paths(expanded_session_path: &str) -> [String; 3] {
        [
            expanded_session_path.to_string(),
            format!("{expanded_session_path}-wal"),
            format!("{expanded_session_path}-shm"),
        ]
    }

    #[cfg(feature = "whatsapp-web")]
    async fn try_transcribe_voice_note(
        client: &wa_rs::Client,
        audio: &wa_rs_proto::whatsapp::message::AudioMessage,
        transcription_config: Option<&crate::config::TranscriptionConfig>,
        transcription_manager: Option<&super::super::pipeline::transcription::TranscriptionManager>,
    ) -> Option<String> {
        let config = transcription_config?;
        let manager = transcription_manager?;

        if let Some(seconds) = audio.seconds {
            if u64::from(seconds) > config.max_duration_secs {
                tracing::info!(
                    "WhatsApp Web: skipping voice note ({}s exceeds {}s limit)",
                    seconds,
                    config.max_duration_secs
                );
                return None;
            }
        }

        use wa_rs::download::Downloadable;
        let audio_data = match client.download(audio as &dyn Downloadable).await {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!("WhatsApp Web: failed to download voice note: {e}");
                return None;
            }
        };

        let file_name = match audio.mimetype.as_deref() {
            Some(m) if m.contains("opus") || m.contains("ogg") => "voice.ogg",
            Some(m) if m.contains("mp4") || m.contains("m4a") => "voice.m4a",
            Some(m) if m.contains("mpeg") || m.contains("mp3") => "voice.mp3",
            Some(m) if m.contains("webm") => "voice.webm",
            _ => "voice.ogg",
        };

        tracing::info!(
            "WhatsApp Web: transcribing voice note ({} bytes, file={})",
            audio_data.len(),
            file_name
        );

        match manager.transcribe(&audio_data, file_name).await {
            Ok(text) if text.trim().is_empty() => {
                tracing::info!("WhatsApp Web: voice transcription returned empty text, skipping");
                None
            }
            Ok(text) => {
                tracing::info!(
                    "WhatsApp Web: voice note transcribed ({} chars)",
                    text.len()
                );
                Some(text)
            }
            Err(e) => {
                tracing::warn!("WhatsApp Web: voice transcription failed: {e}");
                None
            }
        }
    }

    #[cfg(feature = "whatsapp-web")]
    async fn synthesize_voice_static(
        client: &wa_rs::Client,
        to: &wa_rs_binary::jid::Jid,
        text: &str,
        tts_config: &crate::config::TtsConfig,
    ) -> Result<()> {
        let tts_manager = super::super::pipeline::tts::TtsManager::new(tts_config)?;
        let audio_bytes = tts_manager.synthesize(text).await?;
        let audio_len = audio_bytes.len();
        tracing::info!("WhatsApp Web TTS: synthesized {} bytes of audio", audio_len);

        if audio_bytes.is_empty() {
            anyhow::bail!("TTS returned empty audio");
        }

        use wa_rs_core::download::MediaType;
        let upload = client
            .upload(audio_bytes, MediaType::Audio)
            .await
            .map_err(|e| anyhow!("Failed to upload TTS audio: {e}"))?;

        tracing::info!(
            "WhatsApp Web TTS: uploaded audio (url_len={}, file_length={})",
            upload.url.len(),
            upload.file_length
        );

        #[allow(clippy::cast_possible_truncation)]
        let estimated_seconds = std::cmp::max(1, (upload.file_length / 4000) as u32);

        let voice_msg = wa_rs_proto::whatsapp::Message {
            audio_message: Some(Box::new(wa_rs_proto::whatsapp::message::AudioMessage {
                url: Some(upload.url),
                direct_path: Some(upload.direct_path),
                media_key: Some(upload.media_key),
                file_enc_sha256: Some(upload.file_enc_sha256),
                file_sha256: Some(upload.file_sha256),
                file_length: Some(upload.file_length),
                mimetype: Some("audio/ogg; codecs=opus".to_string()),
                ptt: Some(true),
                seconds: Some(estimated_seconds),
                ..Default::default()
            })),
            ..Default::default()
        };

        Box::pin(client.send_message(to.clone(), voice_msg))
            .await
            .map_err(|e| anyhow!("Failed to send voice note: {e}"))?;
        tracing::info!(
            "WhatsApp Web TTS: sent voice note ({} bytes, ~{}s)",
            audio_len,
            estimated_seconds
        );
        Ok(())
    }
}

#[cfg(feature = "whatsapp-web")]
#[async_trait]
impl Channel for WhatsAppWebChannel {
    fn name(&self) -> &str {
        "whatsapp"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {
        let client = self.client.lock().clone();
        let Some(client) = client else {
            anyhow::bail!("WhatsApp Web client not connected. Initialize the bot first.");
        };

        if !Self::is_jid(&message.recipient) {
            let normalized = self.normalize_phone(&message.recipient);
            if !self.is_number_allowed(&normalized) {
                tracing::warn!(
                    "WhatsApp Web: recipient {} not in allowed list",
                    message.recipient
                );
                return Ok(());
            }
        }

        let to = self.recipient_to_jid(&message.recipient)?;

        let is_voice_chat = self
            .voice_chats
            .lock()
            .map(|vs| vs.contains(&message.recipient))
            .unwrap_or(false);

        if is_voice_chat && self.tts_config.is_some() {
            let content = &message.content;

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
                let client_clone = client.clone();
                let to_clone = to.clone();
                let recipient = message.recipient.clone();
                if let Some(tts_config) = self.tts_config.clone() {
                    let _voice_task = crate::runtime::spawn_supervised(
                        "channels.whatsapp.voice_tts",
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
                                match Box::pin(WhatsAppWebChannel::synthesize_voice_static(
                                    &client_clone,
                                    &to_clone,
                                    &text,
                                    &tts_config,
                                ))
                                .await
                                {
                                    Ok(()) => {
                                        tracing::info!(
                                            "WhatsApp Web: voice reply sent ({} chars)",
                                            text.len()
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!("WhatsApp Web: TTS voice reply failed: {e}");
                                    }
                                }
                            }
                        },
                    );
                }
            }

        }

        let outgoing = wa_rs_proto::whatsapp::Message {
            conversation: Some(message.content.clone()),
            ..Default::default()
        };

        let message_id = client.send_message(to, outgoing).await?;
        tracing::debug!(
            "WhatsApp Web: sent text to {} (id: {})",
            message.recipient,
            message_id
        );
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<()> {

        *self.tx.lock() = Some(tx.clone());

        use wa_rs::bot::Bot;
        use wa_rs::pair_code::PairCodeOptions;
        use wa_rs::store::{Device, DeviceStore};
        use wa_rs_binary::jid::JidExt as _;
        use wa_rs_core::proto_helpers::MessageExt;
        use wa_rs_core::types::events::Event;
        use wa_rs_tokio_transport::TokioWebSocketTransportFactory;
        use wa_rs_ureq_http::UreqHttpClient;

        let retry_count = Arc::new(std::sync::atomic::AtomicU32::new(0));

        loop {
            let expanded_session_path = shellexpand::tilde(&self.session_path).to_string();

            tracing::info!(
                "WhatsApp Web channel starting (session: {})",
                expanded_session_path
            );

            let storage = {
                let path = expanded_session_path.clone();
                tokio::task::spawn_blocking(move || RusqliteStore::new(&path))
                    .await
                    .map_err(|e| anyhow::anyhow!("RusqliteStore init task failed: {e}"))??
            };
            let backend = Arc::new(storage);

            let mut device = Device::new(backend.clone());
            if backend.exists().await? {
                tracing::info!("WhatsApp Web: found existing session, loading device");
                if let Some(core_device) = backend.load().await? {
                    device.load_from_serializable(core_device);
                } else {
                    anyhow::bail!("Device exists but failed to load");
                }
            } else {
                tracing::info!(
                    "WhatsApp Web: no existing session, new device will be created during pairing"
                );
            };

            let mut transport_factory = TokioWebSocketTransportFactory::new();
            if let Ok(ws_url) = std::env::var("WHATSAPP_WS_URL") {
                transport_factory = transport_factory.with_url(ws_url);
            }

            let http_client = UreqHttpClient::new();

            let (logout_tx, mut logout_rx) = tokio::sync::broadcast::channel::<()>(1);

            let session_revoked = Arc::new(std::sync::atomic::AtomicBool::new(false));

            let tx_clone = tx.clone();
            let allowed_numbers = self.allowed_numbers.clone();
            let logout_tx_clone = logout_tx.clone();
            let retry_count_clone = retry_count.clone();
            let session_revoked_clone = session_revoked.clone();
            let transcription_config = self.transcription.clone();
            let transcription_mgr = self.transcription_manager.clone();
            let voice_chats = self.voice_chats.clone();
            let wa_mode = self.mode.clone();
            let wa_dm_policy = self.dm_policy.clone();
            let wa_group_policy = self.group_policy.clone();
            let wa_self_chat_mode = self.self_chat_mode;
            let wa_dm_mention_patterns = self.dm_mention_patterns.clone();
            let wa_group_mention_patterns = self.group_mention_patterns.clone();

            let mut builder = Bot::builder()
                .with_backend(backend)
                .with_transport_factory(transport_factory)
                .with_http_client(http_client)
                .with_device_props(
                    Some("SenWeaverCoding".to_string()),
                    None,
                    Some(PlatformType::Desktop),
                )
                .on_event(move |event, client| {
                    let tx_inner = tx_clone.clone();
                    let allowed_numbers = allowed_numbers.clone();
                    let logout_tx = logout_tx_clone.clone();
                    let retry_count = retry_count_clone.clone();
                    let session_revoked = session_revoked_clone.clone();
                    let transcription_config = transcription_config.clone();
                    let transcription_mgr = transcription_mgr.clone();
                    let voice_chats = voice_chats.clone();
                    let wa_mode = wa_mode.clone();
                    let wa_dm_policy = wa_dm_policy.clone();
                    let wa_group_policy = wa_group_policy.clone();
                    let wa_dm_mention_patterns = wa_dm_mention_patterns.clone();
                    let wa_group_mention_patterns = wa_group_mention_patterns.clone();
                    async move {
                        match event {
                            Event::Message(msg, info) => {
                                let sender_jid = info.source.sender.clone();
                                let sender_alt = info.source.sender_alt.clone();
                                let sender = sender_jid.user().to_string();
                                let chat = info.source.chat.to_string();

                                let mapped_phone = if sender_jid.is_lid() {
                                    client.get_phone_number_from_lid(&sender_jid.user).await
                                } else {
                                    None
                                };
                                let sender_candidates = Self::sender_phone_candidates(
                                    &sender_jid,
                                    sender_alt.as_ref(),
                                    mapped_phone.as_deref(),
                                );

                                let normalized = sender_candidates
                                    .iter()
                                    .find(|candidate| {
                                        Self::is_number_allowed_for_list(&allowed_numbers, candidate)
                                    })
                                    .cloned();

                                let is_group = info.source.is_group;

                                if wa_mode == crate::config::WhatsAppWebMode::Personal {

                                    let sender_user = sender_jid.user();
                                    let chat_user = chat
                                        .split_once('@')
                                        .map(|(u, _)| u)
                                        .unwrap_or(&chat);
                                    let is_self_chat = !is_group && sender_user == chat_user && info.source.is_from_me;

                                    if is_self_chat {
                                        if !wa_self_chat_mode {
                                            tracing::debug!(
                                                "WhatsApp Web: ignoring self-chat message (self_chat_mode=false)"
                                            );
                                            return;
                                        }

                                    } else if is_group {
                                        match wa_group_policy {
                                            crate::config::WhatsAppChatPolicy::Ignore => {
                                                tracing::debug!(
                                                    "WhatsApp Web: ignoring group message (group_policy=ignore)"
                                                );
                                                return;
                                            }
                                            crate::config::WhatsAppChatPolicy::All => {

                                            }
                                            crate::config::WhatsAppChatPolicy::Allowlist => {
                                                if normalized.is_none() {
                                                    tracing::warn!(
                                                        "WhatsApp Web: message from unrecognized sender not in allowed list (candidates_count={})",
                                                        sender_candidates.len()
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                    } else {

                                        match wa_dm_policy {
                                            crate::config::WhatsAppChatPolicy::Ignore => {
                                                tracing::debug!(
                                                    "WhatsApp Web: ignoring DM (dm_policy=ignore)"
                                                );
                                                return;
                                            }
                                            crate::config::WhatsAppChatPolicy::All => {

                                            }
                                            crate::config::WhatsAppChatPolicy::Allowlist => {
                                                if normalized.is_none() {
                                                    tracing::warn!(
                                                        "WhatsApp Web: message from unrecognized sender not in allowed list (candidates_count={})",
                                                        sender_candidates.len()
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                    }
                                }

                                let normalized = normalized.unwrap_or_else(|| sender.clone());

                                let voice_text = if let Some(ref audio) = msg.audio_message {
                                    let is_ptt = audio.ptt == Some(true);
                                    let non_ptt_enabled = transcription_config
                                        .as_ref()
                                        .is_some_and(|c| c.transcribe_non_ptt_audio);
                                    if is_ptt || non_ptt_enabled {
                                        Self::try_transcribe_voice_note(
                                            &client,
                                            audio,
                                            transcription_config.as_ref(),
                                            transcription_mgr.as_deref(),
                                        )
                                        .await
                                    } else {
                                        tracing::debug!(
                                            "WhatsApp Web: ignoring non-PTT audio message from {}",
                                            normalized
                                        );
                                        None
                                    }
                                } else {
                                    None
                                };

                                let content = if let Some(ref vt) = voice_text {
                                    if let Ok(mut vs) = voice_chats.lock() {
                                        vs.insert(chat.clone());
                                    }
                                    format!("[Voice] {vt}")
                                } else {
                                    if let Ok(mut vs) = voice_chats.lock() {
                                        vs.remove(&chat);
                                    }
                                    let text = msg.text_content().unwrap_or("");
                                    text.trim().to_string()
                                };

                                tracing::info!(
                                    "WhatsApp Web message received (sender_len={}, chat_len={}, content_len={})",
                                    sender.len(),
                                    chat.len(),
                                    content.len()
                                );
                                tracing::debug!(
                                    "WhatsApp Web message content: {}",
                                    content
                                );

                                if content.is_empty() {
                                    tracing::debug!(
                                        "WhatsApp Web: ignoring empty or non-text message from {}",
                                        normalized
                                    );
                                    return;
                                }

                                let content =
                                    match super::WhatsAppChannel::apply_mention_gating(
                                        &wa_dm_mention_patterns,
                                        &wa_group_mention_patterns,
                                        &content,
                                        is_group,
                                    ) {
                                        Some(c) => c,
                                        None => {
                                            tracing::debug!(
                                                "WhatsApp Web: message from {normalized} did not match mention patterns, dropping"
                                            );
                                            return;
                                        }
                                    };

                                if let Err(e) = tx_inner
                                    .send(ChannelMessage {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        channel: "whatsapp".to_string(),
                                        sender: normalized.clone(),

                                        reply_target: chat,
                                        content,
                                        timestamp: chrono::Utc::now().timestamp() as u64,
                                        thread_ts: None,
                                        interruption_scope_id: None,
                    attachments: vec![],
                                    })
                                    .await
                                {
                                    tracing::error!("Failed to send message to channel: {}", e);
                                }
                            }
                            Event::Connected(_) => {
                                tracing::info!("WhatsApp Web connected successfully");
                                WhatsAppWebChannel::reset_retry(&retry_count);
                            }
                            Event::LoggedOut(_) => {
                                session_revoked.store(true, std::sync::atomic::Ordering::Relaxed);
                                tracing::warn!(
                                    "WhatsApp Web was logged out  -  will clear session and reconnect"
                                );
                                let _ = logout_tx.send(());
                            }
                            Event::StreamError(stream_error) => {
                                tracing::error!("WhatsApp Web stream error: {:?}", stream_error);
                            }
                            Event::PairingCode { code, .. } => {
                                tracing::info!("WhatsApp Web pair code received");
                                tracing::info!(
                                    "Link your phone by entering this code in WhatsApp > Linked Devices"
                                );
                                eprintln!();
                                eprintln!("WhatsApp Web pair code: {code}");
                                eprintln!();
                            }
                            Event::PairingQrCode { code, .. } => {
                                tracing::info!(
                                    "WhatsApp Web QR code received (scan with WhatsApp > Linked Devices)"
                                );
                                match Self::render_pairing_qr(&code) {
                                    Ok(rendered) => {
                                        eprintln!();
                                        eprintln!(
                                            "WhatsApp Web QR code (scan in WhatsApp > Linked Devices):"
                                        );
                                        eprintln!("{rendered}");
                                        eprintln!();
                                    }
                                    Err(err) => {
                                        tracing::warn!(
                                            "WhatsApp Web: failed to render pairing QR in terminal: {}",
                                            err
                                        );
                                        eprintln!();
                                        eprintln!("WhatsApp Web QR payload: {code}");
                                        eprintln!();
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                });

            if let Some(ref phone) = self.pair_phone {
                tracing::info!("WhatsApp Web: pair-code flow enabled for configured phone number");
                builder = builder.with_pair_code(PairCodeOptions {
                    phone_number: phone.clone(),
                    custom_code: self.pair_code.clone(),
                    ..Default::default()
                });
            } else if self.pair_code.is_some() {
                tracing::warn!(
                    "WhatsApp Web: pair_code is set but pair_phone is missing; pair code config is ignored"
                );
            }

            let mut bot = builder.build().await?;
            *self.client.lock() = Some(bot.client());

            let bot_handle = bot.run().await?;

            *self.bot_handle.lock() = Some(bot_handle);

            drop(logout_tx);

            let should_reconnect = select! {
                res = logout_rx.recv() => {

                    let _ = res;
                    true
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("WhatsApp Web channel received Ctrl+C");
                    false
                }
            };

            *self.client.lock() = None;
            let handle = self.bot_handle.lock().take();
            if let Some(handle) = handle {
                handle.abort();

                let _ = handle.await;
            }

            drop(bot);
            drop(device);

            if should_reconnect {
                let (attempts, exceeded) = Self::record_retry(&retry_count);
                if exceeded {
                    anyhow::bail!(
                        "WhatsApp Web: exceeded {} reconnect attempts, giving up",
                        Self::MAX_RETRIES
                    );
                }

                if Self::should_purge_session(&session_revoked) {
                    for path in Self::session_file_paths(&expanded_session_path) {
                        match tokio::fs::remove_file(&path).await {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => tracing::warn!(
                                "WhatsApp Web: failed to remove session file {}: {e}",
                                path
                            ),
                        }
                    }
                    tracing::info!(
                        "WhatsApp Web: session files removed, restarting for QR pairing"
                    );
                } else {
                    tracing::warn!(
                        "WhatsApp Web: bot stopped without LoggedOut; reconnecting with existing session"
                    );
                }

                let delay = Self::compute_retry_delay(attempts);
                tracing::info!(
                    "WhatsApp Web: reconnecting in {}s (attempt {}/{})",
                    delay,
                    attempts,
                    Self::MAX_RETRIES
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                continue;
            }

            break;
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        let bot_handle_guard = self.bot_handle.lock();
        bot_handle_guard.is_some()
    }

    async fn start_typing(&self, recipient: &str) -> Result<()> {
        let client = self.client.lock().clone();
        let Some(client) = client else {
            anyhow::bail!("WhatsApp Web client not connected. Initialize the bot first.");
        };

        if !Self::is_jid(recipient) {
            let normalized = self.normalize_phone(recipient);
            if !self.is_number_allowed(&normalized) {
                tracing::warn!(
                    "WhatsApp Web: typing target {} not in allowed list",
                    recipient
                );
                return Ok(());
            }
        }

        let to = self.recipient_to_jid(recipient)?;
        client
            .chatstate()
            .send_composing(&to)
            .await
            .map_err(|e| anyhow!("Failed to send typing state (composing): {e}"))?;

        tracing::debug!("WhatsApp Web: start typing for {}", recipient);
        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> Result<()> {
        let client = self.client.lock().clone();
        let Some(client) = client else {
            anyhow::bail!("WhatsApp Web client not connected. Initialize the bot first.");
        };

        if !Self::is_jid(recipient) {
            let normalized = self.normalize_phone(recipient);
            if !self.is_number_allowed(&normalized) {
                tracing::warn!(
                    "WhatsApp Web: typing target {} not in allowed list",
                    recipient
                );
                return Ok(());
            }
        }

        let to = self.recipient_to_jid(recipient)?;
        client
            .chatstate()
            .send_paused(&to)
            .await
            .map_err(|e| anyhow!("Failed to send typing state (paused): {e}"))?;

        tracing::debug!("WhatsApp Web: stop typing for {}", recipient);
        Ok(())
    }
}

#[cfg(not(feature = "whatsapp-web"))]
pub struct WhatsAppWebChannel {
    _private: (),
}

#[cfg(not(feature = "whatsapp-web"))]
impl WhatsAppWebChannel {
    pub fn new(
        _session_path: String,
        _pair_phone: Option<String>,
        _pair_code: Option<String>,
        _allowed_numbers: Vec<String>,
        _mode: crate::config::WhatsAppWebMode,
        _dm_policy: crate::config::WhatsAppChatPolicy,
        _group_policy: crate::config::WhatsAppChatPolicy,
        _self_chat_mode: bool,
    ) -> Self {
        Self { _private: () }
    }

    pub fn with_transcription(self, _config: crate::config::TranscriptionConfig) -> Self {
        self
    }

    pub fn with_tts(self, _config: crate::config::TtsConfig) -> Self {
        self
    }
}

#[cfg(not(feature = "whatsapp-web"))]
#[async_trait]
impl Channel for WhatsAppWebChannel {
    fn name(&self) -> &str {
        "whatsapp"
    }

    async fn send(&self, _message: &SendMessage) -> Result<()> {
        anyhow::bail!(
            "WhatsApp Web channel requires the 'whatsapp-web' feature. \
            Enable with: cargo build --features whatsapp-web"
        );
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<()> {
        anyhow::bail!(
            "WhatsApp Web channel requires the 'whatsapp-web' feature. \
            Enable with: cargo build --features whatsapp-web"
        );
    }

    async fn health_check(&self) -> bool {
        false
    }

    async fn start_typing(&self, _recipient: &str) -> Result<()> {
        anyhow::bail!(
            "WhatsApp Web channel requires the 'whatsapp-web' feature. \
            Enable with: cargo build --features whatsapp-web"
        );
    }

    async fn stop_typing(&self, _recipient: &str) -> Result<()> {
        anyhow::bail!(
            "WhatsApp Web channel requires the 'whatsapp-web' feature. \
            Enable with: cargo build --features whatsapp-web"
        );
    }
}
