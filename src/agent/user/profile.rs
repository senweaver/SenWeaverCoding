// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn profile_cache() -> &'static RwLock<HashMap<PathBuf, (u128, String)>> {
    static CACHE: OnceLock<RwLock<HashMap<PathBuf, (u128, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn file_mtime_nanos(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

const USER_PROFILE_FILENAME: &str = "USER.md";
const DEFAULT_PROFILE: &str = "# User Profile\n\n\
Write information about yourself here. The agent will use this context \
to personalize responses.\n\n\
## Preferences\n\n\
- Language: English\n\
- Communication style: Concise\n";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserProfileConfig {

    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default = "default_max_chars")]
    pub max_inject_chars: usize,
}

fn default_enabled() -> bool {
    true
}
fn default_max_chars() -> usize {
    2000
}

impl Default for UserProfileConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_inject_chars: default_max_chars(),
        }
    }
}

pub struct UserProfile {
    config: UserProfileConfig,
    profile_path: PathBuf,
}

impl UserProfile {
    pub fn new(workspace_dir: &Path, config: UserProfileConfig) -> Self {
        Self {
            config,
            profile_path: workspace_dir.join(USER_PROFILE_FILENAME),
        }
    }

    pub fn read(&self) -> Result<String> {
        let meta = match std::fs::metadata(&self.profile_path) {
            Ok(m) => m,
            Err(_) => return Ok(String::new()),
        };
        let mtime = file_mtime_nanos(&meta);
        if let Some((cached_mtime, cached)) =
            profile_cache().read().get(&self.profile_path).cloned()
        {
            if cached_mtime == mtime {
                return Ok(cached);
            }
        }
        let content = std::fs::read_to_string(&self.profile_path)
            .with_context(|| format!("Failed to read {}", self.profile_path.display()))?;
        profile_cache()
            .write()
            .insert(self.profile_path.clone(), (mtime, content.clone()));
        Ok(content)
    }

    pub fn write(&self, content: &str) -> Result<()> {
        if let Some(parent) = self.profile_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::util::atomic_write(&self.profile_path, content.as_bytes())
            .with_context(|| format!("Failed to write {}", self.profile_path.display()))?;
        profile_cache().write().remove(&self.profile_path);
        Ok(())
    }

    pub fn ensure_exists(&self) -> Result<()> {
        if !self.profile_path.exists() {
            self.write(DEFAULT_PROFILE)?;
        }
        Ok(())
    }

    pub fn prompt_injection(&self) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let content = self.read().ok()?;
        if content.trim().is_empty() {
            return None;
        }

        let trimmed = if content.len() > self.config.max_inject_chars {
            let mut end = self.config.max_inject_chars;
            while end > 0 && !content.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &content[..end])
        } else {
            content
        };

        Some(format!(
            "\n<user_profile>\n{}\n</user_profile>\n",
            trimmed.trim()
        ))
    }

    pub fn exists(&self) -> bool {
        self.profile_path.exists()
    }

    pub fn path(&self) -> &Path {
        &self.profile_path
    }
}
