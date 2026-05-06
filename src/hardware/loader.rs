// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Plugin manifest loader — scans `~/.senweavercoding/tools/` at startup.
//!
//! Layout expected on disk:
//! ```text
//! ~/.senweavercoding/tools/
//! ├── i2c_scan/
//! │   ├── tool.toml
//! │   └── i2c_scan.py
//! └── pwm_set/
//!     ├── tool.toml
//!     └── pwm_set
//! ```
//!
//! Rules:
//! - The directory is **created** if it does not exist.
//! - Each subdirectory is scanned for a `tool.toml`.
//! - Manifests that fail to parse or validate are **skipped with a warning**;
//!   they must not crash startup.
//! - Non-directory entries at the top level are silently ignored.

use super::manifest::ToolManifest;
use super::subprocess::SubprocessTool;
use crate::tools::traits::Tool;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub struct LoadedPlugin {

    pub name: String,

    pub version: String,

    pub tool: Box<dyn Tool>,
}

pub fn scan_plugin_dir() -> Vec<LoadedPlugin> {
    let tools_dir = match plugin_tools_dir() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("[registry] cannot resolve plugin tools dir: {}", e);
            return Vec::new();
        }
    };

    if !tools_dir.exists() {
        if let Err(e) = fs::create_dir_all(&tools_dir) {
            tracing::warn!(
                "[registry] could not create {:?}: {}",
                tools_dir.display(),
                e
            );
            return Vec::new();
        }
        tracing::info!(
            "[registry] created plugin directory: {}",
            tools_dir.display()
        );
    }

    println!(
        "[registry] scanning {}...",
        match dirs_home().as_deref().filter(|s| !s.is_empty()) {
            Some(home) => tools_dir
                .to_str()
                .unwrap_or("~/.senweavercoding/tools")
                .replace(home, "~"),
            None => tools_dir
                .to_str()
                .unwrap_or("~/.senweavercoding/tools")
                .to_string(),
        }
    );

    let mut plugins = Vec::new();

    let entries = match fs::read_dir(&tools_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("[registry] cannot read tools dir: {}", e);
            return Vec::new();
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("[registry] skipping unreadable dir entry: {}", e);
                continue;
            }
        };

        let plugin_dir = entry.path();

        if !plugin_dir.is_dir() {
            continue;
        }

        let manifest_path = plugin_dir.join("tool.toml");

        if !manifest_path.exists() {
            tracing::debug!(
                "[registry] no tool.toml in {:?} — skipping",
                plugin_dir.file_name().unwrap_or_default()
            );
            continue;
        }

        match load_one_plugin(&plugin_dir, &manifest_path) {
            Ok(plugin) => plugins.push(plugin),
            Err(e) => {
                tracing::warn!(
                    "[registry] skipping plugin in {:?}: {}",
                    plugin_dir.file_name().unwrap_or_default(),
                    e
                );
            }
        }
    }

    plugins
}

fn load_one_plugin(plugin_dir: &Path, manifest_path: &Path) -> Result<LoadedPlugin> {
    let raw = fs::read_to_string(manifest_path)
        .map_err(|e| anyhow::anyhow!("cannot read tool.toml: {}", e))?;

    let manifest: ToolManifest = toml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("TOML parse error in tool.toml: {}", e))?;

    if manifest.tool.name.trim().is_empty() {
        anyhow::bail!("manifest missing [tool] name");
    }
    if manifest.tool.description.trim().is_empty() {
        anyhow::bail!("manifest missing [tool] description");
    }
    if manifest.exec.binary.trim().is_empty() {
        anyhow::bail!("manifest missing [exec] binary");
    }

    let canonical_plugin_dir = plugin_dir.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "cannot canonicalize plugin dir {}: {}",
            plugin_dir.display(),
            e
        )
    })?;
    let raw_binary_path = plugin_dir.join(&manifest.exec.binary);
    if !raw_binary_path.exists() {
        anyhow::bail!(
            "manifest exec binary not found: {}",
            raw_binary_path.display()
        );
    }
    let binary_path = raw_binary_path.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "cannot canonicalize binary path {}: {}",
            raw_binary_path.display(),
            e
        )
    })?;
    if !binary_path.starts_with(&canonical_plugin_dir) {
        anyhow::bail!(
            "manifest exec binary escapes plugin directory: {} is not under {}",
            binary_path.display(),
            canonical_plugin_dir.display()
        );
    }
    if !binary_path.is_file() {
        anyhow::bail!(
            "manifest exec binary is not a regular file: {}",
            binary_path.display()
        );
    }

    let name = manifest.tool.name.clone();
    let version = manifest.tool.version.clone();
    let tool: Box<dyn Tool> = Box::new(SubprocessTool::new(manifest, binary_path));

    Ok(LoadedPlugin {
        name,
        version,
        tool,
    })
}

pub fn plugin_tools_dir() -> Result<PathBuf> {
    use directories::BaseDirs;
    let base = BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("cannot determine the user home directory"))?;
    Ok(base.home_dir().join(".senweavercoding").join("tools"))
}

fn dirs_home() -> Option<String> {
    use directories::BaseDirs;
    BaseDirs::new().map(|b| b.home_dir().to_string_lossy().into_owned())
}
