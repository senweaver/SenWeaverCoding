// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Keybinding resolver — mirrors claude-code-typescript-src`keybindings/resolver.ts`.

use super::defaults::default_bindings;
use super::parser::ParsedKey;
use super::schema::{KeyAction, KeyBinding};

/// Resolves key events to actions using the active binding set.
pub struct KeybindingResolver {
    bindings: Vec<KeyBinding>,
}

impl KeybindingResolver {
    /// Create a resolver with default bindings.
    pub fn new() -> Self {
        Self {
            bindings: default_bindings(),
        }
    }

    /// Create a resolver with custom bindings merged over defaults.
    pub fn with_overrides(user_bindings: Vec<KeyBinding>) -> Self {
        let mut defaults = default_bindings();
        // User bindings override defaults for the same key+modifiers combo.
        for ub in &user_bindings {
            defaults.retain(|d| !(d.key == ub.key && d.modifiers == ub.modifiers));
        }
        defaults.extend(user_bindings);
        Self { bindings: defaults }
    }

    /// Resolve a parsed key event to an action.
    pub fn resolve(&self, key: &ParsedKey) -> Option<&KeyAction> {
        self.bindings
            .iter()
            .find(|b| {
                b.key.eq_ignore_ascii_case(&key.key)
                    && b.modifiers.len() == key.modifiers.len()
                    && b.modifiers.iter().all(|m| key.modifiers.contains(m))
            })
            .map(|b| &b.action)
    }

    /// List all active bindings.
    pub fn list_bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    /// Add a binding at runtime.
    pub fn add_binding(&mut self, binding: KeyBinding) {
        // Remove any existing binding for the same key combo.
        self.bindings
            .retain(|b| !(b.key == binding.key && b.modifiers == binding.modifiers));
        self.bindings.push(binding);
    }

    /// Remove a binding by action.
    pub fn remove_action(&mut self, action: &KeyAction) {
        self.bindings.retain(|b| &b.action != action);
    }
}

impl Default for KeybindingResolver {
    fn default() -> Self {
        Self::new()
    }
}
