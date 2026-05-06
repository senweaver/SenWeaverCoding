// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Internationalization support for tool descriptions.
//!
//! Loads tool descriptions from TOML locale files in `tool_descriptions/`.
//! Falls back to English when a locale file or specific key is missing,
//! and ultimately falls back to the hardcoded `tool.description()` value
//! if no file-based description exists.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct ToolDescriptions {

    locale_descriptions: HashMap<String, String>,

    english_fallback: HashMap<String, String>,

    locale: String,
}

#[derive(Debug, serde::Deserialize)]
struct DescriptionFile {
    #[serde(default)]
    tools: HashMap<String, String>,
}

impl ToolDescriptions {

    pub fn load(locale: &str, search_dirs: &[PathBuf]) -> Self {
        let locale_descriptions = load_locale_file(locale, search_dirs);

        let english_fallback = if locale == "en" {
            HashMap::new()
        } else {
            load_locale_file("en", search_dirs)
        };

        debug!(
            locale = locale,
            locale_keys = locale_descriptions.len(),
            english_keys = english_fallback.len(),
            "tool descriptions loaded"
        );

        Self {
            locale_descriptions,
            english_fallback,
            locale: locale.to_string(),
        }
    }

    pub fn get(&self, tool_name: &str) -> Option<&str> {
        self.locale_descriptions
            .get(tool_name)
            .or_else(|| self.english_fallback.get(tool_name))
            .map(String::as_str)
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub fn empty() -> Self {
        Self {
            locale_descriptions: HashMap::new(),
            english_fallback: HashMap::new(),
            locale: "en".to_string(),
        }
    }
}

pub fn detect_locale() -> String {
    if let Ok(val) = std::env::var("SEN_LOCALE") {
        let val = val.trim().to_string();
        if !val.is_empty() {
            return normalize_locale(&val);
        }
    }
    for var in &["LANG", "LC_ALL"] {
        if let Ok(val) = std::env::var(var) {
            let locale = normalize_locale(&val);
            if locale != "C" && locale != "POSIX" && !locale.is_empty() {
                return locale;
            }
        }
    }
    "en".to_string()
}

fn normalize_locale(raw: &str) -> String {

    let base = raw.split('.').next().unwrap_or(raw);

    base.replace('_', "-")
}

pub fn default_search_dirs(workspace_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![workspace_dir.to_path_buf()];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !dirs.contains(&manifest_dir) {
        dirs.push(manifest_dir);
    }

    dirs
}

fn load_locale_file(locale: &str, search_dirs: &[PathBuf]) -> HashMap<String, String> {
    let filename = format!("tool_descriptions/{locale}.toml");

    for dir in search_dirs {
        let path = dir.join(&filename);
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<DescriptionFile>(&contents) {
                Ok(parsed) => {
                    debug!(path = %path.display(), keys = parsed.tools.len(), "loaded locale file");
                    return parsed.tools;
                }
                Err(e) => {
                    debug!(path = %path.display(), error = %e, "failed to parse locale file");
                }
            },
            Err(_) => {

            }
        }
    }

    debug!(
        locale = locale,
        "no locale file found in any search directory"
    );
    HashMap::new()
}
