// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Agent Profiles - named agent configurations for multi-persona workflows.
//!
//! Allows creating, loading, and managing named agent profiles with
//! custom system prompts, tool groups, and model preferences.
//! Profiles are stored as TOML files in the workspace.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// An agent profile configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentProfile {
    /// Unique profile name (kebab-case).
    pub name: String,
    /// Human-readable display name.
    #[serde(default)]
    pub display_name: String,
    /// Profile description.
    #[serde(default)]
    pub description: String,
    /// Custom system prompt / soul text.
    #[serde(default)]
    pub system_prompt: String,
    /// Model override (empty = use default).
    #[serde(default)]
    pub model: Option<String>,
    /// Provider override (empty = use default).
    #[serde(default)]
    pub provider: Option<String>,
    /// Temperature override.
    #[serde(default)]
    pub temperature: Option<f64>,
    /// Active tool groups for this profile.
    #[serde(default)]
    pub tool_groups: Vec<String>,
    /// Tools explicitly allowed (empty = all).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Tools explicitly denied.
    #[serde(default)]
    pub denied_tools: Vec<String>,
    /// Maximum tool iterations override.
    #[serde(default)]
    pub max_tool_iterations: Option<usize>,
    /// Custom metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl AgentProfile {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            display_name: name.clone(),
            name,
            description: String::new(),
            system_prompt: String::new(),
            model: None,
            provider: None,
            temperature: None,
            tool_groups: Vec::new(),
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            max_tool_iterations: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

/// Manages agent profiles stored on disk.
pub struct ProfileManager {
    profiles_dir: PathBuf,
}

impl ProfileManager {
    pub fn new(workspace_dir: &Path) -> Self {
        Self {
            profiles_dir: workspace_dir.join("agents"),
        }
    }

    /// Ensure the profiles directory exists.
    pub fn ensure_dir(&self) -> Result<()> {
        if !self.profiles_dir.exists() {
            std::fs::create_dir_all(&self.profiles_dir)
                .context("Failed to create agents directory")?;
        }
        Ok(())
    }

    /// List all agent profiles.
    pub fn list(&self) -> Result<Vec<AgentProfile>> {
        if !self.profiles_dir.exists() {
            return Ok(Vec::new());
        }

        let mut profiles = Vec::new();
        for entry in std::fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let config_path = path.join("agent.toml");
                if config_path.exists() {
                    match self.load_from_file(&config_path) {
                        Ok(profile) => profiles.push(profile),
                        Err(e) => {
                            tracing::warn!(path = %config_path.display(), error = %e, "Failed to load agent profile");
                        }
                    }
                }
            }
        }

        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    /// Get a specific profile by name.
    pub fn get(&self, name: &str) -> Result<Option<AgentProfile>> {
        let config_path = self.profiles_dir.join(name).join("agent.toml");
        if !config_path.exists() {
            return Ok(None);
        }
        self.load_from_file(&config_path).map(Some)
    }

    /// Create or update a profile.
    pub fn save(&self, profile: &AgentProfile) -> Result<()> {
        self.ensure_dir()?;
        let profile_dir = self.profiles_dir.join(&profile.name);
        std::fs::create_dir_all(&profile_dir)?;

        let config_path = profile_dir.join("agent.toml");
        let content =
            toml::to_string_pretty(profile).context("Failed to serialize agent profile")?;
        std::fs::write(&config_path, content).context("Failed to write agent profile")?;

        if !profile.system_prompt.is_empty() {
            let soul_path = profile_dir.join("SOUL.md");
            std::fs::write(&soul_path, &profile.system_prompt)
                .context("Failed to write SOUL.md")?;
        }

        Ok(())
    }

    /// Delete a profile.
    pub fn delete(&self, name: &str) -> Result<bool> {
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            return Ok(false);
        }
        std::fs::remove_dir_all(&profile_dir).context("Failed to delete agent profile")?;
        Ok(true)
    }

    /// Check if a profile name is valid and available.
    pub fn is_name_available(&self, name: &str) -> Result<(bool, String)> {
        if name.is_empty() {
            return Ok((false, "Name cannot be empty".to_string()));
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Ok((
                false,
                "Name must be alphanumeric with hyphens/underscores only".to_string(),
            ));
        }
        if name.len() > 64 {
            return Ok((false, "Name must be 64 characters or fewer".to_string()));
        }

        let profile_dir = self.profiles_dir.join(name);
        if profile_dir.exists() {
            return Ok((false, format!("Profile '{}' already exists", name)));
        }
        Ok((true, "Available".to_string()))
    }

    fn load_from_file(&self, path: &Path) -> Result<AgentProfile> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut profile: AgentProfile = toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;

        // If system_prompt is empty in the TOML, try loading from adjacent SOUL.md
        let soul_path = path.parent().unwrap().join("SOUL.md");
        if soul_path.exists() && profile.system_prompt.is_empty() {
            profile.system_prompt = std::fs::read_to_string(&soul_path).unwrap_or_default();
        }

        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_creation() {
        let profile = AgentProfile::new("test-agent")
            .with_system_prompt("You are a test agent.")
            .with_model("gpt-4");
        assert_eq!(profile.name, "test-agent");
        assert_eq!(profile.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_name_validation() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(dir.path());

        let (valid, _) = manager.is_name_available("good-name").unwrap();
        assert!(valid);

        let (valid, _) = manager.is_name_available("").unwrap();
        assert!(!valid);

        let (valid, _) = manager.is_name_available("bad name!").unwrap();
        assert!(!valid);
    }

    #[test]
    fn test_crud_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(dir.path());

        let profile =
            AgentProfile::new("researcher").with_system_prompt("You are a research agent.");
        manager.save(&profile).unwrap();

        let loaded = manager.get("researcher").unwrap().unwrap();
        assert_eq!(loaded.name, "researcher");

        let list = manager.list().unwrap();
        assert_eq!(list.len(), 1);

        let deleted = manager.delete("researcher").unwrap();
        assert!(deleted);
        assert!(manager.get("researcher").unwrap().is_none());
    }

    #[test]
    fn test_soul_md_loading() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(dir.path());

        let profile = AgentProfile::new("with-soul").with_system_prompt("I have a soul.");
        manager.save(&profile).unwrap();

        // Overwrite agent.toml with empty system_prompt so SOUL.md fallback kicks in
        let mut bare = AgentProfile::new("with-soul");
        bare.system_prompt = String::new();
        let toml_content = toml::to_string_pretty(&bare).unwrap();
        std::fs::write(dir.path().join("agents/with-soul/agent.toml"), toml_content).unwrap();

        let loaded = manager.get("with-soul").unwrap().unwrap();
        assert_eq!(loaded.system_prompt, "I have a soul.");
    }
}
