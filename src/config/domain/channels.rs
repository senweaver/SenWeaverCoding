// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! Additional channel configurations extracted from `schema.rs`.
//!
//! This module contains channel types not yet migrated to `channels_core.rs`.
//! The main `ChannelsConfig` container remains in `schema.rs` for now.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::traits::ChannelConfig;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebhookConfig {

    pub port: u16,

    #[serde(default)]
    pub listen_path: Option<String>,

    #[serde(default)]
    pub send_url: Option<String>,

    #[serde(default)]
    pub send_method: Option<String>,

    #[serde(default)]
    pub auth_header: Option<String>,

    #[serde(default)]
    pub secret: Option<String>,
}

impl ChannelConfig for WebhookConfig {
    fn name() -> &'static str {
        "Webhook"
    }
    fn desc() -> &'static str {
        "HTTP endpoint"
    }
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            listen_path: Some("/webhook".into()),
            send_url: None,
            send_method: Some("POST".into()),
            auth_header: None,
            secret: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IMessageConfig {

    pub allowed_contacts: Vec<String>,
}

impl ChannelConfig for IMessageConfig {
    fn name() -> &'static str {
        "iMessage"
    }
    fn desc() -> &'static str {
        "macOS only"
    }
}

impl Default for IMessageConfig {
    fn default() -> Self {
        Self {
            allowed_contacts: Vec::new(),
        }
    }
}

fn default_channel_message_timeout_secs() -> u64 {
    300
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_session_backend() -> String {
    "sqlite".into()
}
