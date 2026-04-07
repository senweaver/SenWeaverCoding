// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Keybinding schema — mirrors claude-code-typescript-src`keybindings/schema.ts`.

use serde::{Deserialize, Serialize};

/// Keyboard modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyModifier {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

/// Actions that can be triggered by keybindings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    Submit,
    Cancel,
    NewLine,
    HistoryPrev,
    HistoryNext,
    AutoMode,
    PlanMode,
    Compact,
    Clear,
    Help,
    Exit,
    ToggleVim,
    Interrupt,
    TabComplete,
    VoiceToggle,
    Custom(String),
}

/// A keybinding mapping a key sequence to an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: String,
    pub modifiers: Vec<KeyModifier>,
    pub action: KeyAction,
    pub description: String,
    pub when: Option<String>,
}

impl KeyBinding {
    /// Format the binding for display (e.g. "Ctrl+Enter").
    pub fn display_key(&self) -> String {
        let mut parts: Vec<String> = self
            .modifiers
            .iter()
            .map(|m| match m {
                KeyModifier::Ctrl => "Ctrl".to_string(),
                KeyModifier::Alt => "Alt".to_string(),
                KeyModifier::Shift => "Shift".to_string(),
                KeyModifier::Meta => "Meta".to_string(),
            })
            .collect();
        parts.push(self.key.clone());
        parts.join("+")
    }
}
