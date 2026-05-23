// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybinding {
    pub key: String,
    pub action: KeyAction,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    Submit,
    Cancel,
    ClearLine,
    HistoryPrev,
    HistoryNext,
    Complete,
    NewLine,
    Exit,
    ToggleVim,
    Custom(String),
}

pub fn default_keybindings() -> Vec<Keybinding> {
    vec![
        Keybinding {
            key: "Enter".into(),
            action: KeyAction::Submit,
            description: "Submit the current input".into(),
        },
        Keybinding {
            key: "Ctrl+C".into(),
            action: KeyAction::Cancel,
            description: "Cancel current operation".into(),
        },
        Keybinding {
            key: "Ctrl+D".into(),
            action: KeyAction::Exit,
            description: "Exit the session".into(),
        },
        Keybinding {
            key: "Ctrl+U".into(),
            action: KeyAction::ClearLine,
            description: "Clear the current line".into(),
        },
        Keybinding {
            key: "Up".into(),
            action: KeyAction::HistoryPrev,
            description: "Previous history item".into(),
        },
        Keybinding {
            key: "Down".into(),
            action: KeyAction::HistoryNext,
            description: "Next history item".into(),
        },
        Keybinding {
            key: "Tab".into(),
            action: KeyAction::Complete,
            description: "Tab completion".into(),
        },
        Keybinding {
            key: "Shift+Enter".into(),
            action: KeyAction::NewLine,
            description: "Insert a new line".into(),
        },
    ]
}

pub fn validate_keybindings(bindings: &[Keybinding]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen_keys: HashMap<String, &str> = HashMap::new();

    for binding in bindings {
        if let Some(existing) = seen_keys.get(&binding.key) {
            errors.push(format!(
                "Duplicate key '{}': already bound to '{}'",
                binding.key, existing
            ));
        } else {
            seen_keys.insert(binding.key.clone(), binding.description.as_str());
        }
    }

    errors
}
