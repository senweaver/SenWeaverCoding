// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LanConfig {
    #[serde(default = "default_lan_service_name")]
    pub service_name: String,

    #[serde(default = "default_lan_port")]
    pub port: u16,

    #[serde(default = "default_lan_chunk_size")]
    pub chunk_size: usize,

    #[serde(default = "default_lan_max_frame_bytes")]
    pub max_frame_bytes: usize,

    #[serde(default = "default_lan_num_streams")]
    pub num_streams: usize,

    #[serde(default)]
    pub download_dir: Option<String>,
}

pub(crate) fn default_lan_service_name() -> String {
    "_senweaver._tcp.local.".to_string()
}

pub(crate) fn default_lan_port() -> u16 {
    0
}

pub(crate) fn default_lan_chunk_size() -> usize {
    1_048_576
}

pub(crate) fn default_lan_max_frame_bytes() -> usize {
    16_777_216
}

pub(crate) fn default_lan_num_streams() -> usize {
    4
}

impl Default for LanConfig {
    fn default() -> Self {
        Self {
            service_name: default_lan_service_name(),
            port: default_lan_port(),
            chunk_size: default_lan_chunk_size(),
            max_frame_bytes: default_lan_max_frame_bytes(),
            num_streams: default_lan_num_streams(),
            download_dir: None,
        }
    }
}

impl LanConfig {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.chunk_size == 0 {
            errors.push("lan.chunk_size must be >= 1".into());
        }
        if self.max_frame_bytes < self.chunk_size + 4096 {
            errors.push("lan.max_frame_bytes must exceed chunk_size".into());
        }
        errors
    }
}
