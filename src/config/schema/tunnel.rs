// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TunnelConfig {
    pub provider: String,

    #[serde(default)]
    pub cloudflare: Option<CloudflareTunnelConfig>,

    #[serde(default)]
    pub tailscale: Option<TailscaleTunnelConfig>,

    #[serde(default)]
    pub ngrok: Option<NgrokTunnelConfig>,

    #[serde(default)]
    pub openvpn: Option<OpenVpnTunnelConfig>,

    #[serde(default)]
    pub custom: Option<CustomTunnelConfig>,

    #[serde(default)]
    pub pinggy: Option<PinggyTunnelConfig>,
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            provider: "none".into(),
            cloudflare: None,
            tailscale: None,
            ngrok: None,
            openvpn: None,
            custom: None,
            pinggy: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloudflareTunnelConfig {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TailscaleTunnelConfig {
    #[serde(default)]
    pub funnel: bool,

    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NgrokTunnelConfig {
    pub auth_token: String,

    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenVpnTunnelConfig {
    pub config_file: String,

    #[serde(default)]
    pub auth_file: Option<String>,

    #[serde(default)]
    pub advertise_address: Option<String>,

    #[serde(default = "default_openvpn_timeout")]
    pub connect_timeout_secs: u64,

    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_openvpn_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PinggyTunnelConfig {
    #[serde(default)]
    pub token: Option<String>,

    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CustomTunnelConfig {
    pub start_command: String,

    pub health_url: Option<String>,

    pub url_pattern: Option<String>,
}
