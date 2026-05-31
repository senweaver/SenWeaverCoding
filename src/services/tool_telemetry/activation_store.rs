// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolActivationRecord {
    #[serde(rename = "workspaceKey")]
    pub workspace_key: String,
    #[serde(default)]
    pub activated: Vec<String>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: Option<DateTime<Utc>>,
}

pub struct ToolActivationStore {
    base_dir: PathBuf,
    cache: RwLock<std::collections::HashMap<String, ToolActivationRecord>>,
    write_lock: AsyncMutex<()>,
}

impl ToolActivationStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            cache: RwLock::new(std::collections::HashMap::new()),
            write_lock: AsyncMutex::new(()),
        }
    }

    pub fn from_config_path(config_path: &Path) -> Self {
        let parent = config_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::new(parent.join("tool_activations"))
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    fn workspace_file(&self, workspace_key: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(workspace_key.as_bytes());
        let digest = hex::encode(hasher.finalize());
        let hash_prefix: String = digest.chars().take(16).collect();
        self.base_dir.join(format!("{hash_prefix}.json"))
    }

    pub async fn load(&self, workspace_key: &str) -> Result<Vec<String>> {
        if let Some(record) = self.cache.read().get(workspace_key) {
            return Ok(record.activated.clone());
        }
        let path = self.workspace_file(workspace_key);
        let record = if fs::try_exists(&path).await.unwrap_or(false) {
            match fs::read_to_string(&path).await {
                Ok(text) => match serde_json::from_str::<ToolActivationRecord>(&text) {
                    Ok(rec) => rec,
                    Err(e) => {
                        tracing::warn!(
                            target: "tool_activation_store",
                            path = %path.display(),
                            error = %e,
                            "failed to parse tool activation file; ignoring"
                        );
                        ToolActivationRecord {
                            workspace_key: workspace_key.to_string(),
                            ..Default::default()
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        target: "tool_activation_store",
                        path = %path.display(),
                        error = %e,
                        "failed to read tool activation file; ignoring"
                    );
                    ToolActivationRecord {
                        workspace_key: workspace_key.to_string(),
                        ..Default::default()
                    }
                }
            }
        } else {
            ToolActivationRecord {
                workspace_key: workspace_key.to_string(),
                ..Default::default()
            }
        };
        let activated = record.activated.clone();
        self.cache
            .write()
            .insert(workspace_key.to_string(), record);
        Ok(activated)
    }

    pub async fn save(&self, workspace_key: &str, activated: Vec<String>) -> Result<()> {
        let _guard = self.write_lock.lock().await;
        let record = ToolActivationRecord {
            workspace_key: workspace_key.to_string(),
            activated: dedup_keep_order(activated),
            updated_at: Some(Utc::now()),
        };
        let serialized = serde_json::to_string_pretty(&record)
            .context("serialize tool activation record")?;
        let target = self.workspace_file(workspace_key);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create tool activation dir {}", parent.display()))?;
        }
        let tmp_path = target.with_extension("json.tmp");
        fs::write(&tmp_path, serialized.as_bytes())
            .await
            .with_context(|| format!("write tool activation tmp {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &target)
            .await
            .with_context(|| format!("rename tool activation file to {}", target.display()))?;
        self.cache
            .write()
            .insert(workspace_key.to_string(), record);
        Ok(())
    }

    pub async fn add(&self, workspace_key: &str, name: &str) -> Result<bool> {
        let mut current = self.load(workspace_key).await?;
        if current.iter().any(|n| n == name) {
            return Ok(false);
        }
        current.push(name.to_string());
        self.save(workspace_key, current).await?;
        Ok(true)
    }

    pub async fn add_many(&self, workspace_key: &str, names: &[String]) -> Result<usize> {
        if names.is_empty() {
            return Ok(0);
        }
        let mut current = self.load(workspace_key).await?;
        let existing: std::collections::HashSet<&str> =
            current.iter().map(String::as_str).collect();
        let mut added = 0usize;
        let to_append: Vec<String> = names
            .iter()
            .filter(|n| !n.is_empty() && !existing.contains(n.as_str()))
            .cloned()
            .collect();
        if to_append.is_empty() {
            return Ok(0);
        }
        let mut seen: std::collections::HashSet<String> = current.iter().cloned().collect();
        for name in to_append {
            if seen.insert(name.clone()) {
                current.push(name);
                added += 1;
            }
        }
        if added == 0 {
            return Ok(0);
        }
        self.save(workspace_key, current).await?;
        Ok(added)
    }

    pub async fn remove(&self, workspace_key: &str, name: &str) -> Result<bool> {
        let mut current = self.load(workspace_key).await?;
        let before = current.len();
        current.retain(|n| n != name);
        if current.len() == before {
            return Ok(false);
        }
        self.save(workspace_key, current).await?;
        Ok(true)
    }

    pub async fn record(&self, workspace_key: &str) -> Result<ToolActivationRecord> {
        let _ = self.load(workspace_key).await?;
        let rec = self
            .cache
            .read()
            .get(workspace_key)
            .cloned()
            .unwrap_or_else(|| ToolActivationRecord {
                workspace_key: workspace_key.to_string(),
                ..Default::default()
            });
        Ok(rec)
    }
}

fn dedup_keep_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    out
}

pub type ToolActivationStoreHandle = Arc<ToolActivationStore>;
