// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Voice mode configuration — mirrors claude-code-typescript-src`voice/voiceModeEnabled.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceModeConfig {

    pub enabled: bool,

    pub language: String,

    pub push_to_talk_key: Option<String>,

    pub auto_submit: bool,

    pub silence_timeout_ms: u64,

    pub min_confidence: f64,
}

impl Default for VoiceModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            language: "en-US".to_string(),
            push_to_talk_key: None,
            auto_submit: true,
            silence_timeout_ms: 1500,
            min_confidence: 0.7,
        }
    }
}

impl VoiceModeConfig {

    pub fn is_available() -> bool {
        cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        ))
    }
}
