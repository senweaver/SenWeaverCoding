// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Bridge API — HTTP endpoints for bridge management.
// Mirrors claude-code-typescript-src`bridge/bridgeApi.ts` and `bridge/codeSessionApi.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub device_id: String,
    pub device_name: Option<String>,
    pub paircode: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub token: String,
    pub expires_at_epoch_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub session_id: String,
    pub content: String,
    pub attachments: Vec<AttachmentPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentPayload {
    pub filename: String,
    pub media_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusResponse {
    pub session_id: String,
    pub status: String,
    pub agent_status: String,
    pub created_at_epoch_ms: u64,
    pub last_activity_epoch_ms: u64,
    pub message_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeHealthResponse {
    pub status: String,
    pub version: String,
    pub active_sessions: u32,
    pub uptime_secs: u64,
}

pub struct BridgeApi;

impl BridgeApi {

    pub async fn handle_create_session(
        req: CreateSessionRequest,
    ) -> anyhow::Result<CreateSessionResponse> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let token = uuid::Uuid::new_v4().to_string();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let expires_at = now_ms + 24 * 3600 * 1000;

        tracing::info!(
            session_id = %session_id,
            device_id = %req.device_id,
            "Bridge session created"
        );

        Ok(CreateSessionResponse {
            session_id,
            token,
            expires_at_epoch_ms: expires_at,
        })
    }

    pub async fn handle_session_status(session_id: &str) -> anyhow::Result<SessionStatusResponse> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(SessionStatusResponse {
            session_id: session_id.to_string(),
            status: "active".to_string(),
            agent_status: "idle".to_string(),
            created_at_epoch_ms: now_ms,
            last_activity_epoch_ms: now_ms,
            message_count: 0,
        })
    }

    pub async fn handle_health() -> BridgeHealthResponse {
        let uptime = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        BridgeHealthResponse {
            status: "ok".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_sessions: 0,
            uptime_secs: uptime,
        }
    }
}
