// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::traits::ChannelConfig;

use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use reqwest::Client;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub struct TelnyxChannel {

    api_key: String,

    connection_id: String,

    from_number: String,

    allowed_destinations: Vec<String>,

    client: Client,

    #[allow(dead_code)]
    webhook_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TelnyxConfig {

    pub api_key: String,

    pub connection_id: String,

    pub from_number: String,

    #[serde(default)]
    pub allowed_destinations: Vec<String>,

    #[serde(default)]
    pub webhook_secret: Option<String>,
}

impl ChannelConfig for TelnyxConfig {
    fn name() -> &'static str {
        "telnyx"
    }
    fn desc() -> &'static str {
        "Telnyx voice + SMS channel"
    }
}

impl TelnyxChannel {

    pub fn new(config: TelnyxConfig) -> Self {
        Self {
            api_key: config.api_key,
            connection_id: config.connection_id,
            from_number: config.from_number,
            allowed_destinations: config.allowed_destinations,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            webhook_secret: config.webhook_secret,
        }
    }

    const TELNYX_API_URL: &'static str = "https://api.telnyx.com/v2";

    fn is_destination_allowed(&self, destination: &str) -> bool {
        if self.allowed_destinations.is_empty() {
            return true;
        }
        self.allowed_destinations.iter().any(|pattern| {
            pattern == "*" || destination.starts_with(pattern) || pattern == destination
        })
    }

    pub async fn initiate_call(
        &self,
        to: &str,
        _prompt: Option<&str>,
    ) -> anyhow::Result<CallSession> {
        if !self.is_destination_allowed(to) {
            anyhow::bail!("Destination {} is not in allowed list", to);
        }

        let request = CallRequest {
            connection_id: self.connection_id.clone(),
            to: to.to_string(),
            from: self.from_number.clone(),
            answering_machine_detection: Some(AnsweringMachineDetection {
                mode: "premium".to_string(),
            }),
            webhook_url: None,

            command_id: None,
        };

        let response = self
            .client
            .post(format!("{}/calls", Self::TELNYX_API_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to initiate call: {}", error);
        }

        let call_response: CallResponse = response.json().await?;

        Ok(CallSession {
            call_control_id: call_response.call_control_id,
            call_leg_id: call_response.call_leg_id,
            call_session_id: call_response.call_session_id,
        })
    }

    pub async fn speak(&self, call_control_id: &str, text: &str) -> anyhow::Result<()> {
        let request = SpeakRequest {
            payload: text.to_string(),
            payload_type: "text".to_string(),
            service_level: "premium".to_string(),
            voice: "female".to_string(),
            language: "en-US".to_string(),
        };

        let response = self
            .client
            .post(format!(
                "{}/calls/{}/actions/speak",
                Self::TELNYX_API_URL,
                call_control_id
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to speak: {}", error);
        }

        Ok(())
    }

    pub async fn hangup(&self, call_control_id: &str) -> anyhow::Result<()> {
        let response = self
            .client
            .post(format!(
                "{}/calls/{}/actions/hangup",
                Self::TELNYX_API_URL,
                call_control_id
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            tracing::warn!("Failed to hangup call: {}", error);
        }

        Ok(())
    }

    pub async fn start_ai_conversation(
        &self,
        call_control_id: &str,
        system_prompt: &str,
        model: &str,
    ) -> anyhow::Result<()> {
        let request = AiConversationRequest {
            system_prompt: system_prompt.to_string(),
            model: model.to_string(),
            voice_settings: VoiceSettings {
                voice: "alloy".to_string(),
                speed: 1.0,
            },
        };

        let response = self
            .client
            .post(format!(
                "{}/calls/{}/actions/ai_conversation",
                Self::TELNYX_API_URL,
                call_control_id
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to start AI conversation: {}", error);
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CallSession {
    pub call_control_id: String,
    pub call_leg_id: String,
    pub call_session_id: String,
}

#[derive(Debug, Serialize)]
struct CallRequest {
    connection_id: String,
    to: String,
    from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    answering_machine_detection: Option<AnsweringMachineDetection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    webhook_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnsweringMachineDetection {
    mode: String,
}

#[derive(Debug, Deserialize)]
struct CallResponse {
    call_control_id: String,
    call_leg_id: String,
    call_session_id: String,
}

#[derive(Debug, Serialize)]
struct SpeakRequest {
    payload: String,
    payload_type: String,
    service_level: String,
    voice: String,
    language: String,
}

#[derive(Debug, Serialize)]
struct AiConversationRequest {
    system_prompt: String,
    model: String,
    voice_settings: VoiceSettings,
}

#[derive(Debug, Serialize)]
struct VoiceSettings {
    voice: String,
    speed: f32,
}

#[async_trait]
impl Channel for TelnyxChannel {
    fn name(&self) -> &str {
        "telnyx"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        let session = self.initiate_call(&message.recipient, None).await?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        self.speak(&session.call_control_id, &message.content)
            .await?;

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        self.hangup(&session.call_control_id).await?;

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {

        tracing::info!("Telnyx channel listening for incoming calls");

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;

            if tx.is_closed() {
                break;
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {

        let response = self
            .client
            .get(format!("{}/phone_numbers", Self::TELNYX_API_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await;

        match response {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                tracing::warn!("Telnyx health check failed: {}", e);
                false
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TelnyxWebhookEvent {
    pub data: TelnyxWebhookData,
}

#[derive(Debug, Deserialize)]
pub struct TelnyxWebhookData {
    pub event_type: String,
    pub payload: TelnyxCallPayload,
}

#[derive(Debug, Deserialize)]
pub struct TelnyxCallPayload {
    pub call_control_id: Option<String>,
    pub call_leg_id: Option<String>,
    pub call_session_id: Option<String>,
    pub direction: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub state: Option<String>,
}
