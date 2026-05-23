// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::sqlite::SqliteMemory;
use super::traits::{Memory, MemoryCategory, MemoryEntry};
use async_trait::async_trait;
use chrono::Local;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::time::timeout;

pub struct LucidMemory {
    local: SqliteMemory,
    lucid_cmd: String,
    token_budget: usize,
    workspace_dir: PathBuf,
    recall_timeout: Duration,
    store_timeout: Duration,
    local_hit_threshold: usize,
    failure_cooldown: Duration,
    last_failure_at: Mutex<Option<Instant>>,
}

impl LucidMemory {
    const DEFAULT_LUCID_CMD: &'static str = "lucid";
    const DEFAULT_TOKEN_BUDGET: usize = 200;

    const DEFAULT_RECALL_TIMEOUT_MS: u64 = 500;
    const DEFAULT_STORE_TIMEOUT_MS: u64 = 800;
    const DEFAULT_LOCAL_HIT_THRESHOLD: usize = 3;
    const DEFAULT_FAILURE_COOLDOWN_MS: u64 = 15_000;

    pub fn new(workspace_dir: &Path, local: SqliteMemory) -> Self {
        let lucid_cmd =
            std::env::var("SEN_LUCID_CMD").unwrap_or_else(|_| Self::DEFAULT_LUCID_CMD.to_string());

        let token_budget = std::env::var("SEN_LUCID_BUDGET")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(Self::DEFAULT_TOKEN_BUDGET);

        let recall_timeout = Self::read_env_duration_ms(
            "SEN_LUCID_RECALL_TIMEOUT_MS",
            Self::DEFAULT_RECALL_TIMEOUT_MS,
            20,
        );
        let store_timeout = Self::read_env_duration_ms(
            "SEN_LUCID_STORE_TIMEOUT_MS",
            Self::DEFAULT_STORE_TIMEOUT_MS,
            50,
        );
        let local_hit_threshold = Self::read_env_usize(
            "SEN_LUCID_LOCAL_HIT_THRESHOLD",
            Self::DEFAULT_LOCAL_HIT_THRESHOLD,
            1,
        );
        let failure_cooldown = Self::read_env_duration_ms(
            "SEN_LUCID_FAILURE_COOLDOWN_MS",
            Self::DEFAULT_FAILURE_COOLDOWN_MS,
            100,
        );

        Self {
            local,
            lucid_cmd,
            token_budget,
            workspace_dir: workspace_dir.to_path_buf(),
            recall_timeout,
            store_timeout,
            local_hit_threshold,
            failure_cooldown,
            last_failure_at: Mutex::new(None),
        }
    }

    fn read_env_usize(name: &str, default: usize, min: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map_or(default, |v| v.max(min))
    }

    fn read_env_duration_ms(name: &str, default_ms: u64, min_ms: u64) -> Duration {
        let millis = std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(default_ms, |v| v.max(min_ms));
        Duration::from_millis(millis)
    }

    fn in_failure_cooldown(&self) -> bool {
        let guard = self.last_failure_at.lock();
        guard
            .as_ref()
            .is_some_and(|last| last.elapsed() < self.failure_cooldown)
    }

    fn mark_failure_now(&self) {
        let mut guard = self.last_failure_at.lock();
        *guard = Some(Instant::now());
    }

    fn clear_failure(&self) {
        let mut guard = self.last_failure_at.lock();
        *guard = None;
    }

    fn to_lucid_type(category: &MemoryCategory) -> &'static str {
        match category {
            MemoryCategory::Core => "decision",
            MemoryCategory::Daily => "context",
            MemoryCategory::Conversation => "conversation",
            MemoryCategory::Custom(_) => "learning",
        }
    }

    fn to_memory_category(label: &str) -> MemoryCategory {
        let normalized = label.to_lowercase();
        if normalized.contains("visual") {
            return MemoryCategory::Custom("visual".to_string());
        }

        match normalized.as_str() {
            "decision" | "learning" | "solution" => MemoryCategory::Core,
            "context" | "conversation" => MemoryCategory::Conversation,
            "bug" => MemoryCategory::Daily,
            other => MemoryCategory::Custom(other.to_string()),
        }
    }

    fn merge_results(
        primary_results: Vec<MemoryEntry>,
        secondary_results: Vec<MemoryEntry>,
        limit: usize,
    ) -> Vec<MemoryEntry> {
        if limit == 0 {
            return Vec::new();
        }

        let mut merged = Vec::new();
        let mut seen = HashSet::new();

        for entry in primary_results.into_iter().chain(secondary_results) {
            let signature = format!(
                "{}\u{0}{}",
                entry.key.to_lowercase(),
                entry.content.to_lowercase()
            );

            if seen.insert(signature) {
                merged.push(entry);
                if merged.len() >= limit {
                    break;
                }
            }
        }

        merged
    }

    fn parse_lucid_context(raw: &str) -> Vec<MemoryEntry> {
        let mut in_context_block = false;
        let mut entries = Vec::new();
        let now = Local::now().to_rfc3339();

        for line in raw.lines().map(str::trim) {
            if line == "<lucid-context>" {
                in_context_block = true;
                continue;
            }

            if line == "</lucid-context>" {
                break;
            }

            if !in_context_block || line.is_empty() {
                continue;
            }

            let Some(rest) = line.strip_prefix("- [") else {
                continue;
            };

            let Some((label, content_part)) = rest.split_once(']') else {
                continue;
            };

            let content = content_part.trim();
            if content.is_empty() {
                continue;
            }

            let rank = entries.len();
            entries.push(MemoryEntry {
                id: format!("lucid:{rank}"),
                key: format!("lucid_{rank}"),
                content: content.to_string(),
                category: Self::to_memory_category(label.trim()),
                timestamp: now.clone(),
                session_id: None,
                score: Some((1.0 - rank as f64 * 0.05).max(0.1)),
                namespace: "default".into(),
                importance: None,
                superseded_by: None,
            });
        }

        entries
    }

    async fn run_lucid_command_raw(
        lucid_cmd: &str,
        args: &[String],
        timeout_window: Duration,
    ) -> anyhow::Result<String> {
        let mut cmd = crate::util::hidden_async_command(lucid_cmd);
        cmd.args(args);

        let output = timeout(timeout_window, cmd.output()).await.map_err(|_| {
            anyhow::anyhow!(
                "lucid command timed out after {}ms",
                timeout_window.as_millis()
            )
        })??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("lucid command failed: {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn run_lucid_command(
        &self,
        args: &[String],
        timeout_window: Duration,
    ) -> anyhow::Result<String> {
        Self::run_lucid_command_raw(&self.lucid_cmd, args, timeout_window).await
    }

    fn build_store_args(&self, key: &str, content: &str, category: &MemoryCategory) -> Vec<String> {
        let payload = format!("{key}: {content}");
        vec![
            "store".to_string(),
            payload,
            format!("--type={}", Self::to_lucid_type(category)),
            format!("--project={}", self.workspace_dir.display()),
        ]
    }

    fn build_recall_args(&self, query: &str) -> Vec<String> {
        vec![
            "context".to_string(),
            query.to_string(),
            format!("--budget={}", self.token_budget),
            format!("--project={}", self.workspace_dir.display()),
        ]
    }

    async fn sync_to_lucid_async(&self, key: &str, content: &str, category: &MemoryCategory) {
        let args = self.build_store_args(key, content, category);
        if let Err(error) = self.run_lucid_command(&args, self.store_timeout).await {
            tracing::debug!(
                command = %self.lucid_cmd,
                error = %error,
                "Lucid store sync failed; sqlite remains authoritative"
            );
        }
    }

    async fn recall_from_lucid(&self, query: &str) -> anyhow::Result<Vec<MemoryEntry>> {
        let args = self.build_recall_args(query);
        let output = self.run_lucid_command(&args, self.recall_timeout).await?;
        Ok(Self::parse_lucid_context(&output))
    }
}

#[async_trait]
impl Memory for LucidMemory {
    fn name(&self) -> &str {
        "lucid"
    }

    async fn store(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.local
            .store(key, content, category.clone(), session_id)
            .await?;
        self.sync_to_lucid_async(key, content, &category).await;
        Ok(())
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let since_dt = since
            .map(chrono::DateTime::parse_from_rfc3339)
            .transpose()
            .map_err(|e| anyhow::anyhow!("invalid 'since' date (expected RFC 3339): {e}"))?;
        let until_dt = until
            .map(chrono::DateTime::parse_from_rfc3339)
            .transpose()
            .map_err(|e| anyhow::anyhow!("invalid 'until' date (expected RFC 3339): {e}"))?;
        if let (Some(s), Some(u)) = (&since_dt, &until_dt) {
            if s >= u {
                anyhow::bail!("'since' must be before 'until'");
            }
        }

        let local_results = self
            .local
            .recall(query, limit, session_id, since, until)
            .await?;
        if limit == 0
            || local_results.len() >= limit
            || local_results.len() >= self.local_hit_threshold
        {
            return Ok(local_results);
        }

        if self.in_failure_cooldown() {
            return Ok(local_results);
        }

        match self.recall_from_lucid(query).await {
            Ok(lucid_results) if !lucid_results.is_empty() => {
                self.clear_failure();
                let merged = Self::merge_results(local_results, lucid_results, limit);
                let filtered: Vec<MemoryEntry> = merged
                    .into_iter()
                    .filter(|e| {
                        if let Some(ref s) = since_dt {
                            if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&e.timestamp) {
                                if ts < *s {
                                    return false;
                                }
                            }
                        }
                        if let Some(ref u) = until_dt {
                            if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&e.timestamp) {
                                if ts > *u {
                                    return false;
                                }
                            }
                        }
                        true
                    })
                    .collect();
                Ok(filtered)
            }
            Ok(_) => {
                self.clear_failure();
                Ok(local_results)
            }
            Err(error) => {
                self.mark_failure_now();
                tracing::debug!(
                    command = %self.lucid_cmd,
                    error = %error,
                    "Lucid context unavailable; using local sqlite results"
                );
                Ok(local_results)
            }
        }
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        self.local.get(key).await
    }

    async fn list(
        &self,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        self.local.list(category, session_id).await
    }

    async fn forget(&self, key: &str) -> anyhow::Result<bool> {
        self.local.forget(key).await
    }

    async fn count(&self) -> anyhow::Result<usize> {
        self.local.count().await
    }

    async fn health_check(&self) -> bool {
        self.local.health_check().await
    }
}
