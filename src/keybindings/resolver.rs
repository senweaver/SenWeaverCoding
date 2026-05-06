// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Keybinding resolver — mirrors claude-code-typescript-src`keybindings/resolver.ts`.

use super::defaults::default_bindings;
use super::parser::ParsedKey;
use super::schema::{KeyAction, KeyBinding};

#[derive(Clone)]
pub struct KeybindingResolver {
    bindings: Vec<KeyBinding>,
}

impl KeybindingResolver {

    pub fn new() -> Self {
        Self {
            bindings: default_bindings(),
        }
    }

    pub fn with_overrides(user_bindings: Vec<KeyBinding>) -> Self {
        let mut defaults = default_bindings();

        for ub in &user_bindings {
            defaults.retain(|d| !(d.key == ub.key && d.modifiers == ub.modifiers));
        }
        defaults.extend(user_bindings);
        Self { bindings: defaults }
    }

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

    pub fn list_bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    pub fn add_binding(&mut self, binding: KeyBinding) {

        self.bindings
            .retain(|b| !(b.key == binding.key && b.modifiers == binding.modifiers));
        self.bindings.push(binding);
    }

    pub fn remove_action(&mut self, action: &KeyAction) {
        self.bindings.retain(|b| &b.action != action);
    }
}

impl Default for KeybindingResolver {
    fn default() -> Self {
        Self::new()
    }
}
