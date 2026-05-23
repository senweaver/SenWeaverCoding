// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceProfile {

    pub name: String,

    #[serde(default)]
    pub allowed_domains: Vec<String>,

    #[serde(default)]
    pub credential_profile: Option<String>,

    #[serde(default)]
    pub memory_namespace: Option<String>,

    #[serde(default)]
    pub audit_namespace: Option<String>,

    #[serde(default)]
    pub tool_restrictions: Vec<String>,
}

impl WorkspaceProfile {

    pub fn effective_memory_namespace(&self) -> &str {
        self.memory_namespace
            .as_deref()
            .unwrap_or(self.name.as_str())
    }

    pub fn effective_audit_namespace(&self) -> &str {
        self.audit_namespace
            .as_deref()
            .unwrap_or(self.name.as_str())
    }

    pub fn is_tool_restricted(&self, tool_name: &str) -> bool {
        self.tool_restrictions
            .iter()
            .any(|r| r.eq_ignore_ascii_case(tool_name))
    }

    pub fn is_domain_allowed(&self, domain: &str) -> bool {
        if self.allowed_domains.is_empty() {
            return true;
        }
        let domain_lower = domain.to_ascii_lowercase();
        self.allowed_domains
            .iter()
            .any(|d| domain_lower == d.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceManager {

    workspaces_dir: PathBuf,

    profiles: HashMap<String, WorkspaceProfile>,

    active: Option<String>,
}

impl WorkspaceManager {

    pub fn new(workspaces_dir: PathBuf) -> Self {
        Self {
            workspaces_dir,
            profiles: HashMap::new(),
            active: None,
        }
    }

    pub async fn load_profiles(&mut self) -> Result<()> {
        self.profiles.clear();

        let dir = &self.workspaces_dir;
        if !dir.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(dir)
            .await
            .with_context(|| format!("reading workspaces directory: {}", dir.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let profile_path = path.join("profile.toml");
            if !profile_path.exists() {
                continue;
            }
            match tokio::fs::read_to_string(&profile_path).await {
                Ok(contents) => match toml::from_str::<WorkspaceProfile>(&contents) {
                    Ok(profile) => {
                        self.profiles.insert(profile.name.clone(), profile);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "skipping malformed workspace profile {}: {e}",
                            profile_path.display()
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "skipping unreadable workspace profile {}: {e}",
                        profile_path.display()
                    );
                }
            }
        }

        Ok(())
    }

    pub fn switch(&mut self, name: &str) -> Result<&WorkspaceProfile> {
        if !self.profiles.contains_key(name) {
            bail!("workspace '{}' not found", name);
        }
        self.active = Some(name.to_string());
        Ok(&self.profiles[name])
    }

    pub fn active_profile(&self) -> Option<&WorkspaceProfile> {
        self.active
            .as_deref()
            .and_then(|name| self.profiles.get(name))
    }

    pub fn active_name(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    pub fn get(&self, name: &str) -> Option<&WorkspaceProfile> {
        self.profiles.get(name)
    }

    pub async fn create(&mut self, name: &str) -> Result<&WorkspaceProfile> {
        if name.is_empty() {
            bail!("workspace name must not be empty");
        }

        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!(
                "workspace name must contain only alphanumeric characters, hyphens, or underscores"
            );
        }
        if self.profiles.contains_key(name) {
            bail!("workspace '{}' already exists", name);
        }

        let ws_dir = self.workspaces_dir.join(name);
        tokio::fs::create_dir_all(&ws_dir)
            .await
            .with_context(|| format!("creating workspace directory: {}", ws_dir.display()))?;

        let profile = WorkspaceProfile {
            name: name.to_string(),
            allowed_domains: Vec::new(),
            credential_profile: None,
            memory_namespace: Some(name.to_string()),
            audit_namespace: Some(name.to_string()),
            tool_restrictions: Vec::new(),
        };

        let toml_str = toml::to_string_pretty(&profile).context("serializing workspace profile")?;
        let profile_path = ws_dir.join("profile.toml");
        tokio::fs::write(&profile_path, toml_str)
            .await
            .with_context(|| format!("writing workspace profile: {}", profile_path.display()))?;

        self.profiles.insert(name.to_string(), profile);
        Ok(&self.profiles[name])
    }

    pub fn export(&self, name: &str) -> Result<String> {
        let profile = self
            .profiles
            .get(name)
            .with_context(|| format!("workspace '{}' not found", name))?;

        let export = WorkspaceProfile {
            credential_profile: profile
                .credential_profile
                .as_ref()
                .map(|_| "***".to_string()),
            ..profile.clone()
        };

        toml::to_string_pretty(&export).context("serializing workspace profile for export")
    }

    pub fn workspace_dir(&self, name: &str) -> PathBuf {
        self.workspaces_dir.join(name)
    }

    pub fn workspaces_dir(&self) -> &Path {
        &self.workspaces_dir
    }
}
