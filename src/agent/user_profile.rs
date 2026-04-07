// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! User Profile - global user context for agent personalization.
//!
//! Stores user preferences, context, and background information
//! that is injected into every agent session for personalization.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const USER_PROFILE_FILENAME: &str = "USER.md";
const DEFAULT_PROFILE: &str = "# User Profile\n\n\
Write information about yourself here. The agent will use this context \
to personalize responses.\n\n\
## Preferences\n\n\
- Language: English\n\
- Communication style: Concise\n";

/// User profile configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserProfileConfig {
    /// Enable user profile injection into system prompts. Default: true.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Maximum characters to inject from the profile. Default: 2000.
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

/// Manages the global user profile.
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

    /// Read the user profile content.
    pub fn read(&self) -> Result<String> {
        if !self.profile_path.exists() {
            return Ok(String::new());
        }
        std::fs::read_to_string(&self.profile_path)
            .with_context(|| format!("Failed to read {}", self.profile_path.display()))
    }

    /// Write the user profile content.
    pub fn write(&self, content: &str) -> Result<()> {
        if let Some(parent) = self.profile_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.profile_path, content)
            .with_context(|| format!("Failed to write {}", self.profile_path.display()))
    }

    /// Initialize with default content if not present.
    pub fn ensure_exists(&self) -> Result<()> {
        if !self.profile_path.exists() {
            self.write(DEFAULT_PROFILE)?;
        }
        Ok(())
    }

    /// Generate the prompt injection text for the user profile.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_write() {
        let dir = tempfile::tempdir().unwrap();
        let profile = UserProfile::new(dir.path(), UserProfileConfig::default());

        assert!(profile.read().unwrap().is_empty());

        profile.write("# My Profile\nI am a developer.").unwrap();
        let content = profile.read().unwrap();
        assert!(content.contains("developer"));
    }

    #[test]
    fn test_ensure_exists() {
        let dir = tempfile::tempdir().unwrap();
        let profile = UserProfile::new(dir.path(), UserProfileConfig::default());

        assert!(!profile.exists());
        profile.ensure_exists().unwrap();
        assert!(profile.exists());

        let content = profile.read().unwrap();
        assert!(content.contains("User Profile"));
    }

    #[test]
    fn test_prompt_injection() {
        let dir = tempfile::tempdir().unwrap();
        let profile = UserProfile::new(dir.path(), UserProfileConfig::default());
        profile.write("I prefer Rust and concise answers.").unwrap();

        let injection = profile.prompt_injection().unwrap();
        assert!(injection.contains("<user_profile>"));
        assert!(injection.contains("Rust"));
    }

    #[test]
    fn test_disabled_injection() {
        let dir = tempfile::tempdir().unwrap();
        let config = UserProfileConfig {
            enabled: false,
            ..Default::default()
        };
        let profile = UserProfile::new(dir.path(), config);
        profile.write("content").unwrap();

        assert!(profile.prompt_injection().is_none());
    }

    #[test]
    fn test_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let config = UserProfileConfig {
            max_inject_chars: 50,
            ..Default::default()
        };
        let profile = UserProfile::new(dir.path(), config);
        profile.write(&"x".repeat(200)).unwrap();

        let injection = profile.prompt_injection().unwrap();
        assert!(injection.len() < 200);
    }
}
