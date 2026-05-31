// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct HeartbeatConfig {

    pub enabled: bool,

    #[serde(default = "default_heartbeat_interval")]
    pub interval_minutes: u32,

    #[serde(default = "default_decision_before_execute", alias = "two_phase")]
    pub decision_before_execute: bool,

    #[serde(default)]
    pub message: Option<String>,

    #[serde(default, alias = "channel")]
    pub target: Option<String>,

    #[serde(default, alias = "recipient")]
    pub to: Option<String>,

    #[serde(default)]
    pub adaptive: bool,

    #[serde(default = "default_heartbeat_min_interval")]
    pub min_interval_minutes: u32,

    #[serde(default = "default_heartbeat_max_interval")]
    pub max_interval_minutes: u32,

    #[serde(default)]
    pub deadman_timeout_minutes: u32,

    #[serde(default)]
    pub deadman_channel: Option<String>,

    #[serde(default)]
    pub deadman_to: Option<String>,

    #[serde(default = "default_heartbeat_max_run_history")]
    pub max_run_history: u32,

    #[serde(default)]
    pub load_session_context: bool,
}

pub(crate) fn default_heartbeat_interval() -> u32 {
    5
}
pub(crate) fn default_decision_before_execute() -> bool {
    true
}
pub(crate) fn default_heartbeat_min_interval() -> u32 {
    5
}
pub(crate) fn default_heartbeat_max_interval() -> u32 {
    120
}
pub(crate) fn default_heartbeat_max_run_history() -> u32 {
    100
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: default_heartbeat_interval(),
            decision_before_execute: true,
            message: None,
            target: None,
            to: None,
            adaptive: false,
            min_interval_minutes: default_heartbeat_min_interval(),
            max_interval_minutes: default_heartbeat_max_interval(),
            deadman_timeout_minutes: 0,
            deadman_channel: None,
            deadman_to: None,
            max_run_history: default_heartbeat_max_run_history(),
            load_session_context: false,
        }
    }
}

impl HeartbeatConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.enabled && self.interval_minutes == 0 {
            errors.push("heartbeat.interval_minutes must be >= 1 when enabled".into());
        }
        if self.adaptive && self.min_interval_minutes > self.max_interval_minutes {
            errors.push("heartbeat.min_interval_minutes must be <= max_interval_minutes".into());
        }
        if self.adaptive && self.min_interval_minutes == 0 {
            errors.push(
                "heartbeat.min_interval_minutes must be >= 1 when adaptive is enabled".into(),
            );
        }
        if self.deadman_timeout_minutes > 0 && self.deadman_timeout_minutes < self.interval_minutes
        {
            errors.push(
                "heartbeat.deadman_timeout_minutes should be >= interval_minutes to avoid false alarms"
                    .into(),
            );
        }

        match (
            self.target
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            self.to.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        ) {
            (Some(_), None) => errors.push("heartbeat.target set but recipient missing".into()),
            (None, Some(_)) => errors.push("heartbeat.to set but target missing".into()),
            _ => {}
        }
        errors
    }
}
