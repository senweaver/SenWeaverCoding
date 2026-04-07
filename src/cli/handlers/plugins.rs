// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Plugin management handler — install/enable/disable/uninstall plugins.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Plugin metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub path: PathBuf,
    #[serde(default)]
    pub scope: PluginScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginScope {
    #[default]
    Project,
    User,
    System,
}

impl std::fmt::Display for PluginScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Project => write!(f, "project"),
            Self::User => write!(f, "user"),
            Self::System => write!(f, "system"),
        }
    }
}

/// List installed plugins.
pub async fn list_plugins(workspace: &Path) -> Result<Vec<PluginInfo>> {
    let plugins_dir = workspace.join(".senweavercoding").join("plugins");
    if !plugins_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plugins = Vec::new();
    let mut entries = tokio::fs::read_dir(&plugins_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let manifest = entry.path().join("manifest.json");
        if manifest.exists() {
            if let Ok(data) = tokio::fs::read_to_string(&manifest).await {
                if let Ok(info) = serde_json::from_str::<PluginInfo>(&data) {
                    plugins.push(info);
                }
            }
        }
    }

    Ok(plugins)
}

/// Enable a plugin by name.
pub async fn enable_plugin(workspace: &Path, name: &str) -> Result<()> {
    let manifest_path = workspace
        .join(".senweavercoding")
        .join("plugins")
        .join(name)
        .join("manifest.json");
    if !manifest_path.exists() {
        anyhow::bail!("Plugin '{}' not found", name);
    }

    let data = tokio::fs::read_to_string(&manifest_path).await?;
    let mut info: PluginInfo = serde_json::from_str(&data)?;
    info.enabled = true;
    tokio::fs::write(&manifest_path, serde_json::to_string_pretty(&info)?).await?;
    println!("Plugin '{}' enabled", name);
    Ok(())
}

/// Disable a plugin by name.
pub async fn disable_plugin(workspace: &Path, name: &str) -> Result<()> {
    let manifest_path = workspace
        .join(".senweavercoding")
        .join("plugins")
        .join(name)
        .join("manifest.json");
    if !manifest_path.exists() {
        anyhow::bail!("Plugin '{}' not found", name);
    }

    let data = tokio::fs::read_to_string(&manifest_path).await?;
    let mut info: PluginInfo = serde_json::from_str(&data)?;
    info.enabled = false;
    tokio::fs::write(&manifest_path, serde_json::to_string_pretty(&info)?).await?;
    println!("Plugin '{}' disabled", name);
    Ok(())
}

/// Print plugins in a table format.
pub fn print_plugins(plugins: &[PluginInfo]) {
    if plugins.is_empty() {
        println!("No plugins installed.");
        return;
    }

    println!(
        "{:<25} {:<10} {:<10} {:<8} {}",
        "NAME", "VERSION", "SCOPE", "ENABLED", "DESCRIPTION"
    );
    println!("{}", "-".repeat(80));
    for p in plugins {
        println!(
            "{:<25} {:<10} {:<10} {:<8} {}",
            p.name,
            p.version,
            p.scope,
            if p.enabled { "yes" } else { "no" },
            p.description
        );
    }
}
