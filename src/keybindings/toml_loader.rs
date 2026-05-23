// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::resolver::KeybindingResolver;
use super::schema::{KeyAction, KeyBinding, KeyModifier};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingsFile {
    #[serde(default, rename = "binding")]
    pub bindings: Vec<KeyBindingToml>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindingToml {
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<String>,
    pub action: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Debug, Default)]
pub struct LoadReport {
    pub loaded_from: Option<PathBuf>,
    pub accepted: usize,
    pub warnings: Vec<String>,
}

pub fn user_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".sen").join("keybindings.toml"))
}

pub fn load_user_keybindings() -> (KeybindingResolver, LoadReport) {
    let candidates: Vec<PathBuf> = [
        std::env::var_os("SEN_KEYBINDINGS").map(PathBuf::from),
        std::env::current_dir()
            .ok()
            .map(|p| p.join(".sen").join("keybindings.toml")),
        user_config_path(),
    ]
    .into_iter()
    .flatten()
    .filter(|p| p.is_file())
    .collect();

    for path in candidates {
        if let Some((resolver, report)) = try_load_from(&path) {
            record_reload_metric();
            return (resolver, report);
        }
    }
    (KeybindingResolver::new(), LoadReport::default())
}

fn record_reload_metric() {
    if let Some(svc) = crate::services::try_get_services() {
        use crate::observability::agent_metrics::LabelSet;
        svc.agent_metrics
            .inc("sen_keybindings_reload_total", LabelSet::new(vec![]));
    }
}

fn try_load_from(path: &Path) -> Option<(KeybindingResolver, LoadReport)> {
    let mut report = LoadReport {
        loaded_from: Some(path.to_path_buf()),
        ..Default::default()
    };
    let bytes = std::fs::read_to_string(path).ok()?;
    let doc: KeybindingsFile = match toml::from_str(&bytes) {
        Ok(v) => v,
        Err(e) => {
            report.warnings.push(format!("toml parse error: {e}"));
            return Some((KeybindingResolver::new(), report));
        }
    };

    let mut user_bindings = Vec::with_capacity(doc.bindings.len());
    for raw in doc.bindings {
        match convert_binding(raw) {
            Ok(b) => {
                user_bindings.push(b);
                report.accepted += 1;
            }
            Err(w) => report.warnings.push(w),
        }
    }
    Some((KeybindingResolver::with_overrides(user_bindings), report))
}

fn convert_binding(raw: KeyBindingToml) -> Result<KeyBinding, String> {
    let mut modifiers = Vec::with_capacity(raw.modifiers.len());
    for m in raw.modifiers {
        modifiers.push(match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => KeyModifier::Ctrl,
            "alt" | "opt" | "option" => KeyModifier::Alt,
            "shift" => KeyModifier::Shift,
            "meta" | "cmd" | "win" | "super" => KeyModifier::Meta,
            other => return Err(format!("unknown modifier: {other}")),
        });
    }
    let action = match raw.action.to_ascii_lowercase().as_str() {
        "submit" => KeyAction::Submit,
        "cancel" => KeyAction::Cancel,
        "new_line" | "newline" => KeyAction::NewLine,
        "history_prev" => KeyAction::HistoryPrev,
        "history_next" => KeyAction::HistoryNext,
        "auto_mode" => KeyAction::AutoMode,
        "plan_mode" => KeyAction::PlanMode,
        "compact" => KeyAction::Compact,
        "clear" => KeyAction::Clear,
        "help" => KeyAction::Help,
        "exit" | "quit" => KeyAction::Exit,
        "toggle_vim" => KeyAction::ToggleVim,
        "interrupt" => KeyAction::Interrupt,
        "tab_complete" => KeyAction::TabComplete,
        "voice_toggle" => KeyAction::VoiceToggle,
        other => KeyAction::Custom(other.to_string()),
    };
    Ok(KeyBinding {
        key: raw.key,
        modifiers,
        action,
        description: raw.description,
        when: raw.when,
    })
}
