// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Bridge configuration — mirrors claude-code-typescript-src`bridge/bridgeConfig.ts`.

use serde::{Deserialize, Serialize};

use super::types::PollConfig;

/// Configuration for the remote control bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Whether the bridge is enabled.
    pub enabled: bool,
    /// WebSocket server URL for the bridge relay.
    pub relay_url: Option<String>,
    /// Port for the local bridge server (0 = auto).
    pub port: u16,
    /// Host to bind the local bridge server.
    pub host: String,
    /// Whether to start the bridge automatically on session start.
    pub auto_start: bool,
    /// Maximum number of concurrent remote sessions.
    pub max_sessions: u32,
    /// Session timeout in milliseconds (0 = no timeout).
    pub session_timeout_ms: u64,
    /// Poll configuration for reconnection.
    pub poll_config: PollConfig,
    /// Whether to require device pairing.
    pub require_pairing: bool,
    /// JWT secret for session tokens (auto-generated if not set).
    pub jwt_secret: Option<String>,
    /// Trusted device IDs (persisted across sessions).
    pub trusted_devices: Vec<String>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            relay_url: None,
            port: 0,
            host: "127.0.0.1".to_string(),
            auto_start: false,
            max_sessions: 5,
            session_timeout_ms: 3_600_000, // 1 hour
            poll_config: PollConfig::default(),
            require_pairing: true,
            jwt_secret: None,
            trusted_devices: Vec::new(),
        }
    }
}
