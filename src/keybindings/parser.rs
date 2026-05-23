// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::schema::KeyModifier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedKey {
    pub key: String,
    pub modifiers: Vec<KeyModifier>,
}

pub fn parse_key_sequence(input: &str) -> ParsedKey {
    let parts: Vec<&str> = input.split('+').map(|s| s.trim()).collect();
    let mut modifiers = Vec::new();
    let mut key = String::new();

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers.push(KeyModifier::Ctrl),
            "alt" | "option" => modifiers.push(KeyModifier::Alt),
            "shift" => modifiers.push(KeyModifier::Shift),
            "meta" | "cmd" | "command" | "super" | "win" => modifiers.push(KeyModifier::Meta),
            _ => key = part.to_string(),
        }
    }

    ParsedKey { key, modifiers }
}

pub fn normalize_key(key: &str) -> &str {
    match key.to_lowercase().as_str() {
        "return" | "cr" => "Enter",
        "esc" | "escape" => "Escape",
        "bs" | "backspace" => "Backspace",
        "del" | "delete" => "Delete",
        "space" | " " => "Space",
        "tab" => "Tab",
        "up" | "arrowup" => "Up",
        "down" | "arrowdown" => "Down",
        "left" | "arrowleft" => "Left",
        "right" | "arrowright" => "Right",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => key,
    }
}
