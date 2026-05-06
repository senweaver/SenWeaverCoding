// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Voice STT service — mirrors claude-code-typescript-src`services/voice.ts` and `services/voiceStreamSTT.ts`.
// Provides speech-to-text integration for voice input mode.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub enabled: bool,
    pub language: String,
    pub stt_provider: SttProvider,
    pub sample_rate: u32,
    pub channels: u16,
    pub key_terms: Vec<String>,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            language: "en-US".to_string(),
            stt_provider: SttProvider::System,
            sample_rate: 16000,
            channels: 1,
            key_terms: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttProvider {
    System,
    Whisper,
    Deepgram,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub confidence: f64,
    pub language: Option<String>,
    pub duration_ms: u64,
    pub is_final: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Listening,
    Processing,
    Error,
}

pub fn is_voice_available() -> bool {

    cfg!(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    ))
}

pub fn format_key_terms(terms: &[String]) -> String {
    if terms.is_empty() {
        return String::new();
    }
    terms.join(", ")
}
