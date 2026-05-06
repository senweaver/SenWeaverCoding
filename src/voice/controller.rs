// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Voice controller — manages voice input state machine.

use super::config::VoiceModeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Listening,
    Processing,
    Error,
}

pub struct VoiceController {
    config: VoiceModeConfig,
    state: VoiceState,
}

impl VoiceController {
    pub fn new(config: VoiceModeConfig) -> Self {
        Self {
            config,
            state: VoiceState::Idle,
        }
    }

    pub fn state(&self) -> VoiceState {
        self.state
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled && VoiceModeConfig::is_available()
    }

    pub fn start_listening(&mut self) -> anyhow::Result<()> {
        if !self.is_enabled() {
            anyhow::bail!("Voice mode is not enabled or not available");
        }
        self.state = VoiceState::Listening;
        Ok(())
    }

    pub fn stop_listening(&mut self) {
        if self.state == VoiceState::Listening {
            self.state = VoiceState::Processing;
        }
    }

    pub fn reset(&mut self) {
        self.state = VoiceState::Idle;
    }

    pub fn set_error(&mut self) {
        self.state = VoiceState::Error;
    }
}
