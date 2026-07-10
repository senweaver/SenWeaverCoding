// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::error::PluginError;
use super::signature::{self, SignatureMode, VerificationResult};
use super::{PluginCapability, PluginInfo, PluginManifest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct PluginHost {
    plugins_dir: PathBuf,
    loaded: HashMap<String, LoadedPlugin>,
    signature_mode: SignatureMode,
    trusted_publisher_keys: Vec<String>,
    max_plugins: usize,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    wasm_path: PathBuf,
    verification: VerificationResult,
}

impl PluginHost {

    pub fn new(workspace_dir: &Path) -> Result<Self, PluginError> {
        Self::with_security(workspace_dir, SignatureMode::Disabled, Vec::new())
    }

    pub fn from_plugins_config(
        workspace_dir: &Path,
        plugins: &crate::config::schema::PluginsConfig,
    ) -> Result<Self, PluginError> {
        Self::with_limits(
            workspace_dir,
            Self::parse_signature_mode(&plugins.security.signature_mode),
            plugins.security.trusted_publisher_keys.clone(),
            plugins.max_plugins.max(1),
        )
    }

    pub fn with_security(
        workspace_dir: &Path,
        signature_mode: SignatureMode,
        trusted_publisher_keys: Vec<String>,
    ) -> Result<Self, PluginError> {
        Self::with_limits(
            workspace_dir,
            signature_mode,
            trusted_publisher_keys,
            crate::config::schema::PluginsConfig::default().max_plugins,
        )
    }

    fn with_limits(
        workspace_dir: &Path,
        signature_mode: SignatureMode,
        trusted_publisher_keys: Vec<String>,
        max_plugins: usize,
    ) -> Result<Self, PluginError> {
        let plugins_dir = workspace_dir.join("plugins");
        if !plugins_dir.exists() {
            std::fs::create_dir_all(&plugins_dir)?;
        }

        let mut host = Self {
            plugins_dir,
            loaded: HashMap::new(),
            signature_mode,
            trusted_publisher_keys,
            max_plugins,
        };

        host.discover()?;
        Ok(host)
    }

    const DISABLED_MARKER: &'static str = ".disabled";

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), PluginError> {
        let plugin_dir = self.plugins_dir.join(name);
        if !plugin_dir.join("manifest.toml").exists() {
            return Err(PluginError::NotFound(name.to_string()));
        }
        let marker = plugin_dir.join(Self::DISABLED_MARKER);
        if enabled {
            if marker.exists() {
                std::fs::remove_file(&marker)?;
            }
            self.discover()?;
        } else {
            std::fs::write(&marker, b"disabled by user\n")?;
            self.loaded.remove(name);
        }
        Ok(())
    }

    pub fn disabled_plugin_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.plugins_dir) else {
            return names;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && path.join(Self::DISABLED_MARKER).exists()
                && path.join("manifest.toml").exists()
            {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    names.push(name.to_string());
                }
            }
        }
        names.sort();
        names
    }

    pub fn parse_signature_mode(mode: &str) -> SignatureMode {
        match mode.to_lowercase().as_str() {
            "strict" => SignatureMode::Strict,
            "permissive" => SignatureMode::Permissive,
            _ => SignatureMode::Disabled,
        }
    }

    fn discover(&mut self) -> Result<(), PluginError> {
        if !self.plugins_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&self.plugins_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.join(Self::DISABLED_MARKER).exists() {
                    continue;
                }
                let manifest_path = path.join("manifest.toml");
                if manifest_path.exists() {
                    if let Ok(manifest) = self.load_manifest(&manifest_path) {
                        if !self.loaded.contains_key(&manifest.name)
                            && self.loaded.len() >= self.max_plugins
                        {
                            tracing::warn!(
                                plugin = %manifest.name,
                                loaded = self.loaded.len(),
                                max = self.max_plugins,
                                "plugins.max_plugins reached; skipping remaining plugins"
                            );
                            break;
                        }

                        let manifest_toml =
                            std::fs::read_to_string(&manifest_path).unwrap_or_default();
                        match self.verify_plugin_signature(
                            &manifest.name,
                            &manifest_toml,
                            &manifest,
                        ) {
                            Ok(verification) => {
                                let wasm_path = path.join(&manifest.wasm_path);
                                self.loaded.insert(
                                    manifest.name.clone(),
                                    LoadedPlugin {
                                        manifest,
                                        wasm_path,
                                        verification,
                                    },
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    plugin = path.display().to_string(),
                                    error = %e,
                                    "skipping plugin due to signature verification failure"
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn load_manifest(&self, path: &Path) -> Result<PluginManifest, PluginError> {
        let content = std::fs::read_to_string(path)?;
        let manifest: PluginManifest = toml::from_str(&content)?;
        Ok(manifest)
    }

    fn verify_plugin_signature(
        &self,
        name: &str,
        manifest_toml: &str,
        manifest: &PluginManifest,
    ) -> Result<VerificationResult, PluginError> {
        signature::enforce_signature_policy(
            name,
            manifest_toml,
            manifest.signature.as_deref(),
            manifest.publisher_key.as_deref(),
            &self.trusted_publisher_keys,
            self.signature_mode,
        )
    }

    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.loaded
            .values()
            .map(|p| PluginInfo {
                name: p.manifest.name.clone(),
                version: p.manifest.version.clone(),
                description: p.manifest.description.clone(),
                capabilities: p.manifest.capabilities.clone(),
                permissions: p.manifest.permissions.clone(),
                wasm_path: p.wasm_path.clone(),
                loaded: p.wasm_path.exists(),
            })
            .collect()
    }

    pub fn get_plugin(&self, name: &str) -> Option<PluginInfo> {
        self.loaded.get(name).map(|p| PluginInfo {
            name: p.manifest.name.clone(),
            version: p.manifest.version.clone(),
            description: p.manifest.description.clone(),
            capabilities: p.manifest.capabilities.clone(),
            permissions: p.manifest.permissions.clone(),
            wasm_path: p.wasm_path.clone(),
            loaded: p.wasm_path.exists(),
        })
    }

    pub fn install(&mut self, source: &str) -> Result<(), PluginError> {
        let source_path = PathBuf::from(source);
        let manifest_path = if source_path.is_dir() {
            source_path.join("manifest.toml")
        } else {
            source_path.clone()
        };

        if !manifest_path.exists() {
            return Err(PluginError::NotFound(format!(
                "manifest.toml not found at {}",
                manifest_path.display()
            )));
        }

        let manifest = self.load_manifest(&manifest_path)?;
        let source_dir = manifest_path
            .parent()
            .ok_or_else(|| PluginError::InvalidManifest("no parent directory".into()))?;

        let wasm_source = source_dir.join(&manifest.wasm_path);
        if !wasm_source.exists() {
            return Err(PluginError::NotFound(format!(
                "WASM file not found: {}",
                wasm_source.display()
            )));
        }

        if self.loaded.contains_key(&manifest.name) {
            return Err(PluginError::AlreadyLoaded(manifest.name));
        }

        if self.loaded.len() >= self.max_plugins {
            return Err(PluginError::LimitReached {
                loaded: self.loaded.len(),
                max: self.max_plugins,
            });
        }

        let manifest_toml = std::fs::read_to_string(&manifest_path)?;
        let verification =
            self.verify_plugin_signature(&manifest.name, &manifest_toml, &manifest)?;

        let dest_dir = self.plugins_dir.join(&manifest.name);
        std::fs::create_dir_all(&dest_dir)?;

        std::fs::copy(&manifest_path, dest_dir.join("manifest.toml"))?;

        let wasm_dest = dest_dir.join(&manifest.wasm_path);
        if let Some(parent) = wasm_dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&wasm_source, &wasm_dest)?;

        self.loaded.insert(
            manifest.name.clone(),
            LoadedPlugin {
                manifest,
                wasm_path: wasm_dest,
                verification,
            },
        );

        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<(), PluginError> {
        if self.loaded.remove(name).is_none() {
            return Err(PluginError::NotFound(name.to_string()));
        }

        let plugin_dir = self.plugins_dir.join(name);
        if plugin_dir.exists() {
            std::fs::remove_dir_all(plugin_dir)?;
        }

        Ok(())
    }

    pub fn tool_plugins(&self) -> Vec<&PluginManifest> {
        self.loaded
            .values()
            .filter(|p| p.manifest.capabilities.contains(&PluginCapability::Tool))
            .map(|p| &p.manifest)
            .collect()
    }

    pub fn tool_plugin_specs(&self) -> Vec<(String, Option<String>, PathBuf)> {
        self.loaded
            .values()
            .filter(|p| p.manifest.capabilities.contains(&PluginCapability::Tool))
            .map(|p| {
                (
                    p.manifest.name.clone(),
                    p.manifest.description.clone(),
                    p.wasm_path.clone(),
                )
            })
            .collect()
    }

    pub fn channel_plugins(&self) -> Vec<&PluginManifest> {
        self.loaded
            .values()
            .filter(|p| p.manifest.capabilities.contains(&PluginCapability::Channel))
            .map(|p| &p.manifest)
            .collect()
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }
}
