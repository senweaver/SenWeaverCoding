// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Key sequence parser — mirrors claude-code-typescript-src`keybindings/parser.ts`.

use super::schema::KeyModifier;

/// Parsed key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedKey {
    pub key: String,
    pub modifiers: Vec<KeyModifier>,
}

/// Parse a key sequence string like "Ctrl+Shift+Enter" into components.
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

/// Normalize a key name to a canonical form.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_key() {
        let parsed = parse_key_sequence("Enter");
        assert_eq!(parsed.key, "Enter");
        assert!(parsed.modifiers.is_empty());
    }

    #[test]
    fn test_parse_ctrl_key() {
        let parsed = parse_key_sequence("Ctrl+c");
        assert_eq!(parsed.key, "c");
        assert_eq!(parsed.modifiers, vec![KeyModifier::Ctrl]);
    }

    #[test]
    fn test_parse_multi_modifier() {
        let parsed = parse_key_sequence("Ctrl+Shift+Enter");
        assert_eq!(parsed.key, "Enter");
        assert_eq!(
            parsed.modifiers,
            vec![KeyModifier::Ctrl, KeyModifier::Shift]
        );
    }
}
