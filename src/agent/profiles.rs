// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentProfile {

    pub name: String,

    #[serde(default)]
    pub display_name: String,

    #[serde(default)]
    pub description: String,

    #[serde(default)]
    pub system_prompt: String,

    #[serde(default)]
    pub model: Option<String>,

    #[serde(default)]
    pub provider: Option<String>,

    #[serde(default)]
    pub temperature: Option<f64>,

    #[serde(default)]
    pub tool_groups: Vec<String>,

    #[serde(default)]
    pub allowed_tools: Vec<String>,

    #[serde(default)]
    pub denied_tools: Vec<String>,

    #[serde(default)]
    pub max_tool_iterations: Option<usize>,

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

pub struct ProfileManager {
    profiles_dir: PathBuf,
}

impl ProfileManager {
    pub fn new(workspace_dir: &Path) -> Self {
        Self {
            profiles_dir: workspace_dir.join("agents"),
        }
    }

    pub fn ensure_dir(&self) -> Result<()> {
        if !self.profiles_dir.exists() {
            std::fs::create_dir_all(&self.profiles_dir)
                .context("Failed to create agents directory")?;
        }
        Ok(())
    }

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

    pub fn get(&self, name: &str) -> Result<Option<AgentProfile>> {
        let config_path = self.profiles_dir.join(name).join("agent.toml");
        if !config_path.exists() {
            return Ok(None);
        }
        self.load_from_file(&config_path).map(Some)
    }

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

    pub fn delete(&self, name: &str) -> Result<bool> {
        let profile_dir = self.profiles_dir.join(name);
        if !profile_dir.exists() {
            return Ok(false);
        }
        std::fs::remove_dir_all(&profile_dir).context("Failed to delete agent profile")?;
        Ok(true)
    }

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

        let soul_path = path
            .parent()
            .map(|p| p.join("SOUL.md"))
            .unwrap_or_else(|| std::path::PathBuf::from("SOUL.md"));
        if soul_path.exists() && profile.system_prompt.is_empty() {
            profile.system_prompt = std::fs::read_to_string(&soul_path).unwrap_or_default();
        }

        Ok(profile)
    }
}
