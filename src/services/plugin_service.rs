// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Plugin lifecycle service — mirrors claude-code-typescript-src`services/plugins/`.
// Manages plugin discovery, loading, enabling/disabling, and health checks.

use std::collections::HashMap;
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
}

impl PluginService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PluginServiceInner {
                plugins: HashMap::new(),
            })),
        }
    }

    pub async fn register(&self, info: PluginInfo) {
        let mut inner = self.inner.write().await;
        inner.plugins.insert(info.name.clone(), info);
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
