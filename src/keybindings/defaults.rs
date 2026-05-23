// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::schema::{KeyAction, KeyBinding, KeyModifier};

pub fn default_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding {
            key: "Enter".to_string(),
            modifiers: vec![],
            action: KeyAction::Submit,
            description: "Submit the current input".to_string(),
            when: None,
        },
        KeyBinding {
            key: "c".to_string(),
            modifiers: vec![KeyModifier::Ctrl],
            action: KeyAction::Interrupt,
            description: "Interrupt current operation".to_string(),
            when: None,
        },
        KeyBinding {
            key: "d".to_string(),
            modifiers: vec![KeyModifier::Ctrl],
            action: KeyAction::Exit,
            description: "Exit the session".to_string(),
            when: None,
        },
        KeyBinding {
            key: "l".to_string(),
            modifiers: vec![KeyModifier::Ctrl],
            action: KeyAction::Clear,
            description: "Clear the screen".to_string(),
            when: None,
        },
        KeyBinding {
            key: "Up".to_string(),
            modifiers: vec![],
            action: KeyAction::HistoryPrev,
            description: "Previous history entry".to_string(),
            when: None,
        },
        KeyBinding {
            key: "Down".to_string(),
            modifiers: vec![],
            action: KeyAction::HistoryNext,
            description: "Next history entry".to_string(),
            when: None,
        },
        KeyBinding {
            key: "Enter".to_string(),
            modifiers: vec![KeyModifier::Shift],
            action: KeyAction::NewLine,
            description: "Insert a new line".to_string(),
            when: None,
        },
        KeyBinding {
            key: "Tab".to_string(),
            modifiers: vec![KeyModifier::Shift],
            action: KeyAction::AutoMode,
            description: "Toggle auto mode".to_string(),
            when: None,
        },
        KeyBinding {
            key: "Tab".to_string(),
            modifiers: vec![],
            action: KeyAction::TabComplete,
            description: "Tab completion".to_string(),
            when: None,
        },
        KeyBinding {
            key: "v".to_string(),
            modifiers: vec![KeyModifier::Ctrl],
            action: KeyAction::ToggleVim,
            description: "Toggle vim mode".to_string(),
            when: None,
        },
    ]
}
