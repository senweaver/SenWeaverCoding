// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::schema::StreamMode;

fn default_true() -> bool {
    true
}
fn default_draft_update_interval_ms() -> u64 {
    1500
}
fn default_slack_draft_update_interval_ms() -> u64 {
    1200
}
fn default_multi_message_delay_ms() -> u64 {
    800
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TelegramConfig {

    pub bot_token: String,

    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub stream_mode: StreamMode,

    #[serde(default = "default_draft_update_interval_ms")]
    pub draft_update_interval_ms: u64,

    #[serde(default)]
    pub interrupt_on_new_message: bool,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default)]
    pub ack_reactions: Option<bool>,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscordConfig {

    pub bot_token: String,

    pub guild_id: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub listen_to_bots: bool,

    #[serde(default)]
    pub interrupt_on_new_message: bool,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default)]
    pub proxy_url: Option<String>,

    #[serde(default)]
    pub stream_mode: StreamMode,

    #[serde(default = "default_draft_update_interval_ms")]
    pub draft_update_interval_ms: u64,

    #[serde(default = "default_multi_message_delay_ms")]
    pub multi_message_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscordHistoryConfig {
    pub bot_token: String,
    pub guild_id: Option<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub channel_ids: Vec<String>,
    #[serde(default = "default_true")]
    pub store_dms: bool,
    #[serde(default = "default_true")]
    pub respond_to_dms: bool,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: Option<String>,
    pub channel_id: Option<String>,
    #[serde(default)]
    pub channel_ids: Vec<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub interrupt_on_new_message: bool,
    #[serde(default)]
    pub thread_replies: Option<bool>,
    #[serde(default)]
    pub mention_only: bool,
    #[serde(default)]
    pub use_markdown_blocks: bool,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub stream_drafts: bool,
    #[serde(default = "default_slack_draft_update_interval_ms")]
    pub draft_update_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MattermostConfig {

    pub url: String,

    pub bot_token: String,

    pub channel_id: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub thread_replies: Option<bool>,

    #[serde(default)]
    pub mention_only: Option<bool>,

    #[serde(default)]
    pub interrupt_on_new_message: bool,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

use crate::config::traits::ChannelConfig;

impl ChannelConfig for TelegramConfig {
    fn name() -> &'static str {
        "Telegram"
    }
    fn desc() -> &'static str {
        "connect your bot"
    }
}
impl ChannelConfig for DiscordConfig {
    fn name() -> &'static str {
        "Discord"
    }
    fn desc() -> &'static str {
        "connect your bot"
    }
}
impl ChannelConfig for DiscordHistoryConfig {
    fn name() -> &'static str {
        "Discord History"
    }
    fn desc() -> &'static str {
        "log all messages and forward @mentions"
    }
}
impl ChannelConfig for SlackConfig {
    fn name() -> &'static str {
        "Slack"
    }
    fn desc() -> &'static str {
        "connect your bot"
    }
}
impl ChannelConfig for MattermostConfig {
    fn name() -> &'static str {
        "Mattermost"
    }
    fn desc() -> &'static str {
        "connect to your bot"
    }
}
