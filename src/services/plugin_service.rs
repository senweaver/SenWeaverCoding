// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub source: PluginSource,
    pub status: PluginStatus,
    pub provides_tools: Vec<String>,
    pub provides_commands: Vec<String>,
    pub provides_hooks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    Builtin,
    Local { path: String },
    Registry { url: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    Enabled,
    Disabled,
    Error,
    Loading,
    NotInstalled,
}

#[derive(Clone)]
pub struct PluginService {
    inner: Arc<RwLock<PluginServiceInner>>,
}

struct PluginServiceInner {
    plugins: HashMap<String, PluginInfo>,
    workspace: Option<PathBuf>,
    plugins_dir: Option<PathBuf>,
}

impl PluginService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PluginServiceInner {
                plugins: HashMap::new(),
                workspace: None,
                plugins_dir: None,
            })),
        }
    }

    pub async fn bind_workspace(&self, workspace: &Path, plugins_dir: Option<PathBuf>) {
        let mut inner = self.inner.write().await;
        inner.workspace = Some(workspace.to_path_buf());
        inner.plugins_dir = plugins_dir;
    }

    pub async fn register(&self, info: PluginInfo) {
        let mut inner = self.inner.write().await;
        inner.plugins.insert(info.name.clone(), info);
    }

    #[cfg(feature = "plugins-wasm")]
    pub async fn sync_from_host(&self, host: &crate::plugins::host::PluginHost) {
        let mut plugins = HashMap::new();
        for p in host.list_plugins() {
            let provides_tools = if p
                .capabilities
                .iter()
                .any(|c| matches!(c, crate::plugins::PluginCapability::Tool))
            {
                vec![p.name.clone()]
            } else {
                Vec::new()
            };
            plugins.insert(
                p.name.clone(),
                PluginInfo {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    description: p.description.clone().unwrap_or_default(),
                    author: String::new(),
                    source: PluginSource::Local {
                        path: p.wasm_path.display().to_string(),
                    },
                    status: if p.loaded {
                        PluginStatus::Enabled
                    } else {
                        PluginStatus::Error
                    },
                    provides_tools,
                    provides_commands: Vec::new(),
                    provides_hooks: Vec::new(),
                },
            );
        }
        for name in host.disabled_plugin_names() {
            plugins
                .entry(name.clone())
                .and_modify(|info| info.status = PluginStatus::Disabled)
                .or_insert(PluginInfo {
                    name: name.clone(),
                    version: String::new(),
                    description: String::new(),
                    author: String::new(),
                    source: PluginSource::Local {
                        path: host.plugins_dir().join(&name).display().to_string(),
                    },
                    status: PluginStatus::Disabled,
                    provides_tools: Vec::new(),
                    provides_commands: Vec::new(),
                    provides_hooks: Vec::new(),
                });
        }
        let mut inner = self.inner.write().await;
        inner.plugins_dir = Some(host.plugins_dir().to_path_buf());
        inner.plugins = plugins;
    }

    #[cfg(feature = "plugins-wasm")]
    pub async fn refresh_from_config(
        &self,
        workspace: &Path,
        plugins: &crate::config::schema::PluginsConfig,
    ) -> anyhow::Result<()> {
        let host = crate::plugins::host::PluginHost::from_plugins_config(workspace, plugins)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.bind_workspace(workspace, Some(host.plugins_dir().to_path_buf()))
            .await;
        self.sync_from_host(&host).await;
        Ok(())
    }

    #[cfg(feature = "plugins-wasm")]
    pub async fn set_enabled_via_host(
        &self,
        workspace: &Path,
        plugins: &crate::config::schema::PluginsConfig,
        name: &str,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let mut host = crate::plugins::host::PluginHost::from_plugins_config(workspace, plugins)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        host.set_enabled(name, enabled)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.sync_from_host(&host).await;
        Ok(())
    }

    pub async fn enable(&self, name: &str) -> bool {
        let mut inner = self.inner.write().await;
        if let Some(plugin) = inner.plugins.get_mut(name) {
            plugin.status = PluginStatus::Enabled;
            return true;
        }
        false
    }

    pub async fn disable(&self, name: &str) -> bool {
        let mut inner = self.inner.write().await;
        if let Some(plugin) = inner.plugins.get_mut(name) {
            plugin.status = PluginStatus::Disabled;
            return true;
        }
        false
    }

    pub async fn list(&self) -> Vec<PluginInfo> {
        let inner = self.inner.read().await;
        inner.plugins.values().cloned().collect()
    }

    pub async fn list_enabled(&self) -> Vec<PluginInfo> {
        let inner = self.inner.read().await;
        inner
            .plugins
            .values()
            .filter(|p| p.status == PluginStatus::Enabled)
            .cloned()
            .collect()
    }

    pub async fn get(&self, name: &str) -> Option<PluginInfo> {
        let inner = self.inner.read().await;
        inner.plugins.get(name).cloned()
    }

    pub async fn remove(&self, name: &str) -> bool {
        let mut inner = self.inner.write().await;
        inner.plugins.remove(name).is_some()
    }

    pub async fn provided_tools(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner
            .plugins
            .values()
            .filter(|p| p.status == PluginStatus::Enabled)
            .flat_map(|p| p.provides_tools.iter().cloned())
            .collect()
    }

    pub async fn provided_commands(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner
            .plugins
            .values()
            .filter(|p| p.status == PluginStatus::Enabled)
            .flat_map(|p| p.provides_commands.iter().cloned())
            .collect()
    }
}

impl Default for PluginService {
    fn default() -> Self {
        Self::new()
    }
}
