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

pub mod toml_loader;

#[doc(inline)]
#[allow(unused_imports)]
pub use toml_loader::{LoadReport, load_user_keybindings, user_config_path};

use std::sync::{Arc, OnceLock};

use self::resolver::KeybindingResolver;

static GLOBAL_RESOLVER: OnceLock<Arc<KeybindingResolver>> = OnceLock::new();

pub fn set_global_resolver(resolver: KeybindingResolver) -> Arc<KeybindingResolver> {
    GLOBAL_RESOLVER.get_or_init(|| Arc::new(resolver)).clone()
}

pub fn install_global_resolver_from_disk() -> Arc<KeybindingResolver> {
    if let Some(existing) = GLOBAL_RESOLVER.get() {
        return existing.clone();
    }
    let (resolver, report) = load_user_keybindings();
    if let Some(path) = &report.loaded_from {
        tracing::info!(
            path = %path.display(),
            accepted = report.accepted,
            warnings = report.warnings.len(),
            "keybindings: loaded TOML"
        );
    } else {
        tracing::debug!("keybindings: no TOML found, using built-in defaults");
    }
    for warning in &report.warnings {
        tracing::warn!(warning = %warning, "keybindings: TOML warning");
    }
    set_global_resolver(resolver)
}

pub fn global_resolver() -> Option<Arc<KeybindingResolver>> {
    GLOBAL_RESOLVER.get().cloned()
}
