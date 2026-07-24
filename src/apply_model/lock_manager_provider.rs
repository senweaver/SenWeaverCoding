// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::time::Duration;

use async_trait::async_trait;

use crate::agent::coordination::{
    AcquireOpts, CoordinatorHandle, LockError, RegionLockTokens, RegionRequest,
};

use super::ops_applier::{LockGuard, LockProvider, LockProviderError, RegionLockRequest};

const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct LockManagerProvider {
    coordinator: CoordinatorHandle,
    holder_id: String,
    default_acquire_timeout: Duration,
}

impl LockManagerProvider {

    pub fn new(coordinator: CoordinatorHandle, holder_id: impl Into<String>) -> Self {
        Self {
            coordinator,
            holder_id: holder_id.into(),
            default_acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
        }
    }

    pub fn with_acquire_timeout(mut self, timeout: Duration) -> Self {
        self.default_acquire_timeout = timeout;
        self
    }
}

pub struct RegionLockGuard {
    _tokens: RegionLockTokens,
}

impl LockGuard for RegionLockGuard {}

#[async_trait]
impl LockProvider for LockManagerProvider {
    async fn acquire_for_paths(
        &self,
        paths: &[std::path::PathBuf],
        holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError> {

        let specs: Vec<RegionRequest> = paths
            .iter()
            .map(|p| RegionRequest {
                path: p.clone(),
                range: 0..usize::MAX,
                exclusive: true,
            })
            .collect();
        self.acquire_specs(specs, holder).await
    }

    async fn acquire_for_regions(
        &self,
        regions: &[RegionLockRequest],
        holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError> {
        let specs: Vec<RegionRequest> = regions
            .iter()
            .map(|r| RegionRequest {
                path: r.path.clone(),
                range: r.range.clone(),
                exclusive: r.exclusive,
            })
            .collect();
        self.acquire_specs(specs, holder).await
    }
}

impl LockManagerProvider {
    async fn acquire_specs(
        &self,
        specs: Vec<RegionRequest>,
        holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError> {
        let opts = AcquireOpts {
            wait_timeout: self.default_acquire_timeout,
            deadlock_detect: true,
            ttl: None,
        };

        let lock_manager = self.coordinator.locks_arc();

        let session = crate::session::resource_lock::current_session_context()
            .map(|ctx| ctx.session_id)
            .filter(|id| !id.trim().is_empty());
        let holder_string = match (session, holder.is_empty()) {
            (Some(session), true) => format!("{}/{}", self.holder_id, session),
            (Some(session), false) => format!("{}/{}/{}", self.holder_id, session, holder),
            (None, true) => self.holder_id.clone(),
            (None, false) => format!("{}/{}", self.holder_id, holder),
        };

        let result = tokio::task::spawn_blocking(move || {
            lock_manager.acquire_multi(&specs, &holder_string, opts)
        })
        .await
        .map_err(|e| LockProviderError::Acquire(format!("lock acquire task failed: {e}")))?;

        match result {
            Ok(tokens) => Ok(Box::new(RegionLockGuard { _tokens: tokens })),
            Err(err) => Err(LockProviderError::Acquire(format_lock_error(&err))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LazyRuntimeLockProvider {
    holder_id: &'static str,
}

impl LazyRuntimeLockProvider {
    #[must_use]
    pub fn new(holder_id: &'static str) -> Self {
        Self { holder_id }
    }
}

impl Default for LazyRuntimeLockProvider {
    fn default() -> Self {
        Self::new("edit_path")
    }
}

#[async_trait]
impl LockProvider for LazyRuntimeLockProvider {
    async fn acquire_for_paths(
        &self,
        paths: &[std::path::PathBuf],
        holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError> {
        match crate::agent::multi_agent_runtime::global_runtime() {
            Some(rt) => {
                LockManagerProvider::new(rt.coordinator.clone(), self.holder_id)
                    .acquire_for_paths(paths, holder)
                    .await
            }
            None => Ok(Box::new(super::ops_applier::NoopLockGuard)),
        }
    }

    async fn acquire_for_regions(
        &self,
        regions: &[RegionLockRequest],
        holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError> {
        match crate::agent::multi_agent_runtime::global_runtime() {
            Some(rt) => {
                LockManagerProvider::new(rt.coordinator.clone(), self.holder_id)
                    .acquire_for_regions(regions, holder)
                    .await
            }
            None => Ok(Box::new(super::ops_applier::NoopLockGuard)),
        }
    }
}

fn format_lock_error(err: &LockError) -> String {
    match err {
        LockError::Conflict { path, holder, range } => format!(
            "lock_conflict: {}@{}..{} held by {}",
            path.display(),
            range.start,
            range.end,
            holder
        ),
        LockError::Deadlock { cycle } => {
            format!("deadlock: {}", cycle.join(" → "))
        }
        LockError::Timeout { path, range } => format!(
            "timeout: {}@{}..{}",
            path.display(),
            range.start,
            range.end
        ),
        LockError::WorkspaceEscape => "workspace_escape".to_string(),
    }
}

