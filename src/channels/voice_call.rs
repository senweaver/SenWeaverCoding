// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, info, warn};

use super::traits::{Channel, ChannelMessage, SendMessage};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VoiceProvider {
    #[default]
    Twilio,
    Telnyx,
    Plivo,
}

impl fmt::Display for VoiceProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Twilio => write!(f, "twilio"),
            Self::Telnyx => write!(f, "telnyx"),
            Self::Plivo => write!(f, "plivo"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VoiceCallConfig {

    #[serde(default)]
    pub provider: VoiceProvider,

    pub account_id: String,

    pub auth_token: String,

    pub from_number: String,

    #[serde(default = "default_webhook_port")]
    pub webhook_port: u16,

    #[serde(default = "default_true")]
    pub require_outbound_approval: bool,

    #[serde(default = "default_true")]
    pub transcription_logging: bool,

    #[serde(default)]
    pub tts_voice: Option<String>,

    #[serde(default = "default_max_call_duration")]
    pub max_call_duration_secs: u64,

    #[serde(default)]
    pub webhook_base_url: Option<String>,
}

fn default_webhook_port() -> u16 {
    8090
}

fn default_true() -> bool {
    true
}

fn default_max_call_duration() -> u64 {
    3600
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallState {

    Ringing,

    InProgress,

    Completed,

    Failed,

    HungUp,

    PendingApproval,
}

impl fmt::Display for CallState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ringing => write!(f, "ringing"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::HungUp => write!(f, "hung_up"),
            Self::PendingApproval => write!(f, "pending_approval"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRecord {

    pub call_id: String,

    pub direction: CallDirection,

    pub remote_number: String,

    pub local_number: String,

    pub state: CallState,

    pub started_at: String,

    pub ended_at: Option<String>,

    pub duration_secs: u64,

    pub transcript: Vec<TranscriptEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {

    pub speaker: String,

    pub text: String,

    pub timestamp: String,
}

pub struct VoiceCallChannel {
    config: VoiceCallConfig,
    active_calls: Arc<Mutex<HashMap<String, CallRecord>>>,
    client: reqwest::Client,
}

impl VoiceCallChannel {
    pub fn new(config: VoiceCallConfig) -> Self {
        Self {
            config,
            active_calls: Arc::new(Mutex::new(HashMap::new())),
            client: reqwest::Client::new(),
        }
    }

    fn api_base_url(&self) -> &str {
        match self.config.provider {
            VoiceProvider::Twilio => "https://api.twilio.com/2010-04-01",
            VoiceProvider::Telnyx => "https://api.telnyx.com/v2",
            VoiceProvider::Plivo => "https://api.plivo.com/v1",
        }
    }

    pub async fn place_call(&self, to_number: &str) -> Result<String> {
        if self.config.require_outbound_approval {
            info!(to = to_number, "outbound call requires approval");
            return Ok(format!("PENDING_APPROVAL:{to_number}"));
        }
        self.execute_outbound_call(to_number).await
    }

    async fn execute_outbound_call(&self, to_number: &str) -> Result<String> {
        let webhook_url = self.webhook_url("/voice/status");

        match self.config.provider {
            VoiceProvider::Twilio => {
                let url = format!(
                    "{}/Accounts/{}/Calls.json",
                    self.api_base_url(),
                    self.config.account_id
                );
                let resp = self
                    .client
                    .post(&url)
                    .basic_auth(&self.config.account_id, Some(&self.config.auth_token))
                    .form(&[
                        ("To", to_number),
                        ("From", &self.config.from_number),
                        ("StatusCallback", &webhook_url),
                        ("Timeout", &self.config.max_call_duration_secs.to_string()),
                    ])
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    bail!("Twilio call failed: {body}");
                }

                let json: serde_json::Value = serde_json::from_str(&resp.text().await?)?;
                let call_sid = json["sid"].as_str().unwrap_or("unknown").to_string();
                info!(call_sid = %call_sid, to = to_number, "outbound call placed via Twilio");
                Ok(call_sid)
            }
            VoiceProvider::Telnyx => {
                let url = format!("{}/calls", self.api_base_url());
                let resp = self
                    .client
                    .post(&url)
                    .bearer_auth(&self.config.auth_token)
                    .json(&serde_json::json!({
                        "connection_id": self.config.account_id,
                        "to": to_number,
                        "from": self.config.from_number,
                        "webhook_url": webhook_url,
                        "timeout_secs": self.config.max_call_duration_secs,
                    }))
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    bail!("Telnyx call failed: {body}");
                }

                let json: serde_json::Value = serde_json::from_str(&resp.text().await?)?;
                let call_id = json["data"]["call_control_id"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                info!(call_id = %call_id, to = to_number, "outbound call placed via Telnyx");
                Ok(call_id)
            }
            VoiceProvider::Plivo => {
                let url = format!(
                    "{}/Account/{}/Call/",
                    self.api_base_url(),
                    self.config.account_id
                );
                let resp = self
                    .client
                    .post(&url)
                    .basic_auth(&self.config.account_id, Some(&self.config.auth_token))
                    .json(&serde_json::json!({
                        "to": to_number,
                        "from": self.config.from_number,
                        "answer_url": self.webhook_url("/voice/answer"),
                        "hangup_url": self.webhook_url("/voice/hangup"),
                        "time_limit": self.config.max_call_duration_secs,
                    }))
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    bail!("Plivo call failed: {body}");
                }

                let json: serde_json::Value = serde_json::from_str(&resp.text().await?)?;
                let call_uuid = json["request_uuid"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                info!(call_uuid = %call_uuid, to = to_number, "outbound call placed via Plivo");
                Ok(call_uuid)
            }
        }
    }

    fn webhook_url(&self, path: &str) -> String {
        if let Some(ref base) = self.config.webhook_base_url {
            format!("{}{}", base.trim_end_matches('/'), path)
        } else {
            format!("http://localhost:{}{}", self.config.webhook_port, path)
        }
    }

    pub async fn add_transcript_entry(&self, call_id: &str, speaker: &str, text: &str) {
        let mut calls = self.active_calls.lock().await;
        if let Some(record) = calls.get_mut(call_id) {
            record.transcript.push(TranscriptEntry {
                speaker: speaker.to_string(),
                text: text.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
    }

    pub async fn get_call(&self, call_id: &str) -> Option<CallRecord> {
        let calls = self.active_calls.lock().await;
        calls.get(call_id).cloned()
    }

    pub async fn active_calls(&self) -> Vec<CallRecord> {
        let calls = self.active_calls.lock().await;
        calls.values().cloned().collect()
    }

    pub async fn handle_inbound_call(
        &self,
        call_id: &str,
        from_number: &str,
        tx: &mpsc::Sender<ChannelMessage>,
    ) -> Result<()> {
        let record = CallRecord {
            call_id: call_id.to_string(),
            direction: CallDirection::Inbound,
            remote_number: from_number.to_string(),
            local_number: self.config.from_number.clone(),
            state: CallState::Ringing,
            started_at: chrono::Utc::now().to_rfc3339(),
            ended_at: None,
            duration_secs: 0,
            transcript: Vec::new(),
        };

        {
            let mut calls = self.active_calls.lock().await;
            calls.insert(call_id.to_string(), record);
        }

        info!(
            call_id = call_id,
            from = from_number,
            "inbound call received"
        );

        let msg = ChannelMessage {
            id: call_id.to_string(),
            sender: from_number.to_string(),
            reply_target: from_number.to_string(),
            content: format!("[Voice Call] Incoming call from {from_number} (call_id: {call_id})"),
            channel: "voice_call".to_string(),
            timestamp: chrono::Utc::now().timestamp().unsigned_abs(),
            thread_ts: Some(call_id.to_string()),
            interruption_scope_id: Some(call_id.to_string()),
            attachments: vec![],
        };
        tx.send(msg)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send call event: {e}"))?;
        Ok(())
    }

    pub async fn handle_status_update(&self, call_id: &str, new_state: CallState) {
        let mut calls = self.active_calls.lock().await;
        if let Some(record) = calls.get_mut(call_id) {
            let old_state = record.state;
            record.state = new_state;

            if matches!(
                new_state,
                CallState::Completed | CallState::Failed | CallState::HungUp
            ) {
                record.ended_at = Some(chrono::Utc::now().to_rfc3339());
            }

            debug!(
                call_id = call_id,
                old_state = %old_state,
                new_state = %new_state,
                "call state transition"
            );
        }
    }

    pub async fn save_transcript(
        &self,
        call_id: &str,
        workspace_dir: &std::path::Path,
    ) -> Result<()> {
        if !self.config.transcription_logging {
            return Ok(());
        }

        let calls = self.active_calls.lock().await;
        let Some(record) = calls.get(call_id) else {
            bail!("Call not found: {call_id}");
        };

        let logs_dir = workspace_dir.join("logs").join("calls");
        std::fs::create_dir_all(&logs_dir)?;

        let filename = format!("{}_{}.json", record.started_at.replace(':', "-"), call_id);
        let path = logs_dir.join(filename);
        let json = serde_json::to_string_pretty(record)?;
        std::fs::write(&path, json)?;

        info!(call_id = call_id, path = %path.display(), "call transcript saved");
        Ok(())
    }
}

#[async_trait::async_trait]
impl Channel for VoiceCallChannel {
    fn name(&self) -> &str {
        "voice_call"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {

        if let Some(ref thread_ts) = message.thread_ts {
            let calls = self.active_calls.lock().await;
            if let Some(record) = calls.get(thread_ts) {
                if record.state == CallState::InProgress {
                    debug!(
                        call_id = thread_ts,
                        "would TTS message to active call: {}", message.content
                    );

                    return Ok(());
                }
            }
        }

        debug!("voice_call send (no active call): {}", message.content);
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        let port = self.config.webhook_port;
        let active_calls = self.active_calls.clone();
        let _tx = tx.clone();

        info!(port = port, provider = %self.config.provider, "voice call webhook server starting");

        let app = axum::Router::new()
            .route("/voice/health", axum::routing::get(|| async { "ok" }))
            .with_state(active_calls);

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind voice webhook server: {e}"))?;

        axum::serve(listener, app)
            .await
            .map_err(|e| anyhow::anyhow!("Voice webhook server error: {e}"))?;

        Ok(())
    }

    async fn health_check(&self) -> bool {

        let test_url = match self.config.provider {
            VoiceProvider::Twilio => {
                format!(
                    "{}/Accounts/{}.json",
                    self.api_base_url(),
                    self.config.account_id
                )
            }
            VoiceProvider::Telnyx => format!("{}/connections", self.api_base_url()),
            VoiceProvider::Plivo => {
                format!(
                    "{}/Account/{}/",
                    self.api_base_url(),
                    self.config.account_id
                )
            }
        };

        match self.client.get(&test_url).send().await {
            Ok(resp) => {

                resp.status().is_success() || resp.status().as_u16() == 401
            }
            Err(e) => {
                warn!(provider = %self.config.provider, "voice call health check failed: {e}");
                false
            }
        }
    }

    async fn start_typing(&self, _recipient: &str) -> Result<()> {
        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> Result<()> {
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        false
    }

    async fn send_draft(&self, _message: &SendMessage) -> Result<Option<String>> {
        Ok(None)
    }

    async fn update_draft(&self, _recipient: &str, _message_id: &str, _text: &str) -> Result<()> {
        Ok(())
    }

    async fn finalize_draft(&self, _recipient: &str, _message_id: &str, _text: &str) -> Result<()> {
        Ok(())
    }

    async fn cancel_draft(&self, _recipient: &str, _message_id: &str) -> Result<()> {
        Ok(())
    }

    async fn add_reaction(&self, _channel_id: &str, _message_id: &str, _emoji: &str) -> Result<()> {
        Ok(())
    }

    async fn remove_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn pin_message(&self, _channel_id: &str, _message_id: &str) -> Result<()> {
        Ok(())
    }

    async fn unpin_message(&self, _channel_id: &str, _message_id: &str) -> Result<()> {
        Ok(())
    }

    async fn redact_message(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _reason: Option<String>,
    ) -> Result<()> {
        Ok(())
    }
}
