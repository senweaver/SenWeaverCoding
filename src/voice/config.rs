// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Voice mode configuration — mirrors claude-code-typescript-src`voice/voiceModeEnabled.ts`.

use serde::{Deserialize, Serialize};

/// Voice mode configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceModeConfig {
    /// Whether voice mode is enabled.
    pub enabled: bool,
    /// Language for speech recognition (BCP-47 tag).
    pub language: String,
    /// Push-to-talk key binding.
    pub push_to_talk_key: Option<String>,
    /// Whether to auto-submit after speech ends.
    pub auto_submit: bool,
    /// Silence timeout before auto-submit (milliseconds).
    pub silence_timeout_ms: u64,
    /// Minimum confidence threshold for transcription.
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
    /// Check if voice mode can be activated on this platform.
    pub fn is_available() -> bool {
        cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        ))
    }
}
