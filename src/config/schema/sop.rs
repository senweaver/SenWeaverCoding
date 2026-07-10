// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SopConfig {

    #[serde(default)]
    pub sops_dir: Option<String>,

    #[serde(default = "default_sop_execution_mode")]
    pub default_execution_mode: String,

    #[serde(default = "default_sop_max_concurrent_total")]
    pub max_concurrent_total: usize,

    #[serde(default = "default_sop_approval_timeout_secs")]
    pub approval_timeout_secs: u64,

    #[serde(default = "default_sop_max_finished_runs")]
    pub max_finished_runs: usize,

    #[serde(default)]
    pub mqtt: Option<MqttConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MqttConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_mqtt_broker_url")]
    pub broker_url: String,

    #[serde(default = "default_mqtt_client_id")]
    pub client_id: String,

    #[serde(default)]
    pub topics: Vec<String>,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub password: Option<String>,

    #[serde(default = "default_mqtt_keep_alive_secs")]
    pub keep_alive_secs: u64,

    #[serde(default = "default_mqtt_qos")]
    pub qos: u8,

    #[serde(default)]
    pub use_tls: bool,
}

fn default_mqtt_broker_url() -> String {
    "mqtt://localhost:1883".to_string()
}

fn default_mqtt_client_id() -> String {
    "sen-sop-listener".to_string()
}

fn default_mqtt_keep_alive_secs() -> u64 {
    30
}

fn default_mqtt_qos() -> u8 {
    1
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            broker_url: default_mqtt_broker_url(),
            client_id: default_mqtt_client_id(),
            topics: Vec::new(),
            username: None,
            password: None,
            keep_alive_secs: default_mqtt_keep_alive_secs(),
            qos: default_mqtt_qos(),
            use_tls: false,
        }
    }
}

impl MqttConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.broker_url.trim().is_empty() {
            anyhow::bail!("mqtt.broker_url must not be empty");
        }
        if self.client_id.trim().is_empty() {
            anyhow::bail!("mqtt.client_id must not be empty");
        }
        if self.topics.is_empty() {
            anyhow::bail!("mqtt.topics must contain at least one topic to subscribe to");
        }
        if self.qos > 2 {
            anyhow::bail!("mqtt.qos must be 0, 1, or 2 (got {})", self.qos);
        }
        Ok(())
    }
}

fn default_sop_execution_mode() -> String {
    "supervised".to_string()
}

fn default_sop_max_concurrent_total() -> usize {
    4
}

fn default_sop_approval_timeout_secs() -> u64 {
    300
}

fn default_sop_max_finished_runs() -> usize {
    100
}

impl Default for SopConfig {
    fn default() -> Self {
        Self {
            sops_dir: None,
            default_execution_mode: default_sop_execution_mode(),
            max_concurrent_total: default_sop_max_concurrent_total(),
            approval_timeout_secs: default_sop_approval_timeout_secs(),
            max_finished_runs: default_sop_max_finished_runs(),
            mqtt: None,
        }
    }
}

