// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportFilter {
    pub namespace: Option<String>,
    pub session_id: Option<String>,
    pub category: Option<MemoryCategory>,

    pub since: Option<String>,

    pub until: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProceduralMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub key: String,
    pub content: String,
    pub category: MemoryCategory,
    pub timestamp: String,
    pub session_id: Option<String>,
    pub score: Option<f64>,

    #[serde(default = "default_namespace")]
    pub namespace: String,

    #[serde(default)]
    pub importance: Option<f64>,

    #[serde(default)]
    pub superseded_by: Option<String>,
}

fn default_namespace() -> String {
    "default".into()
}

impl std::fmt::Debug for MemoryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryEntry")
            .field("id", &self.id)
            .field("key", &self.key)
            .field("content", &self.content)
            .field("category", &self.category)
            .field("timestamp", &self.timestamp)
            .field("score", &self.score)
            .field("namespace", &self.namespace)
            .field("importance", &self.importance)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryCategory {

    Core,

    Daily,

    Conversation,

    Custom(String),
}

impl serde::Serialize for MemoryCategory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for MemoryCategory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "core" => Self::Core,
            "daily" => Self::Daily,
            "conversation" => Self::Conversation,
            _ => Self::Custom(s),
        })
    }
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core => write!(f, "core"),
            Self::Daily => write!(f, "daily"),
            Self::Conversation => write!(f, "conversation"),
            Self::Custom(name) => write!(f, "{name}"),
        }
    }
}

#[async_trait]
pub trait Memory: Send + Sync {

    fn name(&self) -> &str;

    async fn store(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()>;

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>>;

    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>>;

    async fn list(
        &self,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>>;

    async fn forget(&self, key: &str, include_global: bool) -> anyhow::Result<bool>;

    async fn forget_everywhere(&self, key: &str) -> anyhow::Result<bool> {
        self.forget(key, true).await
    }

    async fn purge_namespace(&self, _namespace: &str) -> anyhow::Result<usize> {
        anyhow::bail!("purge_namespace not supported by this memory backend")
    }

    async fn purge_session(&self, _session_id: &str) -> anyhow::Result<usize> {
        anyhow::bail!("purge_session not supported by this memory backend")
    }

    async fn count(&self) -> anyhow::Result<usize>;

    async fn health_check(&self) -> bool;

    async fn store_procedural(
        &self,
        _messages: &[ProceduralMessage],
        _session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn reembed_all(&self) -> anyhow::Result<usize> {
        anyhow::bail!(
            "the '{}' memory backend does not support re-embedding; only the sqlite backend can migrate stored vectors",
            self.name()
        )
    }

    async fn recall_namespaced(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self
            .recall(query, limit * 2, session_id, since, until)
            .await?;
        let filtered: Vec<MemoryEntry> = entries
            .into_iter()
            .filter(|e| e.namespace == namespace)
            .take(limit)
            .collect();
        Ok(filtered)
    }

    async fn export(&self, filter: &ExportFilter) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self
            .list(filter.category.as_ref(), filter.session_id.as_deref())
            .await?;
        let filtered: Vec<MemoryEntry> = entries
            .into_iter()
            .filter(|e| {
                if let Some(ref ns) = filter.namespace {
                    if e.namespace != *ns {
                        return false;
                    }
                }
                if let Some(ref since) = filter.since {
                    if e.timestamp.as_str() < since.as_str() {
                        return false;
                    }
                }
                if let Some(ref until) = filter.until {
                    if e.timestamp.as_str() > until.as_str() {
                        return false;
                    }
                }
                true
            })
            .collect();
        Ok(filtered)
    }

    async fn store_with_metadata(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        _namespace: Option<&str>,
        _importance: Option<f64>,
    ) -> anyhow::Result<()> {
        self.store(key, content, category, session_id).await
    }
}
