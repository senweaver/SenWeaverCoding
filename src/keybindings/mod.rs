// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Keybindings module — mirrors claude-code's `keybindings/` directory.
// Provides keyboard shortcut management: default bindings, user overrides,
// key parsing, and action resolution.

pub mod defaults;
pub mod flat;
pub mod parser;
pub mod resolver;
pub mod schema;

pub use defaults::default_bindings;
pub use flat::{default_keybindings, validate_keybindings};
pub use parser::parse_key_sequence;
pub use resolver::KeybindingResolver;
pub use schema::{KeyAction, KeyBinding, KeyModifier};
