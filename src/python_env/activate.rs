// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use super::cache::load_state;
use super::manager::{PythonEnvState, PythonInterpreterTool};

#[derive(Debug, Clone)]
pub struct ActivationEnv {
    pub assignments: Vec<(String, String)>,
    pub unset: Vec<String>,
}

impl ActivationEnv {
    pub fn into_kv(self) -> Vec<(String, String)> {
        self.assignments
    }
}

pub fn activation_env(workspace: &Path) -> Vec<(String, String)> {
    activation_env_detailed(workspace).assignments
}

pub fn activation_env_detailed(workspace: &Path) -> ActivationEnv {
    let unset = vec!["PYTHONHOME".to_string()];
    let mut assignments: Vec<(String, String)> = Vec::new();

    let Some(state) = load_state(workspace) else {
        return ActivationEnv { assignments, unset };
    };

    let interpreter = match &state.interpreter_path {
        Some(p) if p.is_file() => p.clone(),
        Some(_) => {
            let ws = workspace.to_path_buf();
            std::thread::spawn(move || {
                super::manager::heal_missing_interpreter(&ws);
            });
            return ActivationEnv { assignments, unset };
        }
        _ => return ActivationEnv { assignments, unset },
    };

    if state.is_isolated {
        if let Some(venv_dir) = venv_dir_from_interpreter(&interpreter) {
            assignments.push((
                "VIRTUAL_ENV".to_string(),
                venv_dir.to_string_lossy().to_string(),
            ));
            assignments.push((
                "UV_PROJECT_ENVIRONMENT".to_string(),
                venv_dir.to_string_lossy().to_string(),
            ));
            if let Some(new_path) = prepend_path(&interpreter_bin_dir(&venv_dir)) {
                assignments.push(("PATH".to_string(), new_path));
            }
        }
    } else if state.tool == PythonInterpreterTool::System {
        if let Some(parent) = interpreter.parent() {
            if let Some(new_path) = prepend_path(parent) {
                assignments.push(("PATH".to_string(), new_path));
            }
        }
    }

    let _ = state;

    ActivationEnv { assignments, unset }
}

pub fn venv_dir_from_interpreter(interpreter: &Path) -> Option<PathBuf> {
    let parent = interpreter.parent()?;
    let name = parent
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name == "scripts" || name == "bin" {
        return parent.parent().map(Path::to_path_buf);
    }
    Some(parent.to_path_buf())
}

fn interpreter_bin_dir(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    }
}

fn prepend_path(prefix: &Path) -> Option<String> {
    let prefix_str = prefix.to_string_lossy().to_string();
    if prefix_str.is_empty() {
        return None;
    }
    let existing = std::env::var_os("PATH").map(|s| s.to_string_lossy().to_string());
    let sep = if cfg!(windows) { ";" } else { ":" };
    let new_value = match existing {
        Some(orig) if !orig.is_empty() => format!("{prefix_str}{sep}{orig}"),
        _ => prefix_str,
    };
    Some(new_value)
}

pub fn current_state(workspace: &Path) -> Option<PythonEnvState> {
    load_state(workspace)
}
