// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

use super::types::PollConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {

    pub enabled: bool,

    pub relay_url: Option<String>,

    pub port: u16,

    pub host: String,

    pub auto_start: bool,

    pub max_sessions: u32,

    pub session_timeout_ms: u64,

    pub poll_config: PollConfig,

    pub require_pairing: bool,

    pub jwt_secret: Option<String>,

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
            session_timeout_ms: 3_600_000,
            poll_config: PollConfig::default(),
            require_pairing: true,
            jwt_secret: None,
            trusted_devices: Vec::new(),
        }
    }
}
