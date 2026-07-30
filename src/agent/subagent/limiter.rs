// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentLimitConfig {

    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    #[serde(default = "default_queue_excess")]
    pub queue_excess: bool,
}

fn default_max_concurrent() -> usize {
    3
}
fn default_queue_excess() -> bool {
    true
}

impl Default for SubagentLimitConfig {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent(),
            queue_excess: default_queue_excess(),
        }
    }
}

#[derive(Clone)]
pub struct SubagentLimiter {
    state: Arc<LimiterState>,
    queue_excess: bool,
    lineage: Arc<Mutex<LineageTable>>,
}

struct LimiterState {
    semaphore: Arc<tokio::sync::Semaphore>,
    max_concurrent: AtomicUsize,
    deficit: AtomicUsize,
    active: AtomicUsize,
}

#[derive(Default)]
struct LineageTable {

    parents: HashMap<String, Option<String>>,

    children: HashMap<String, HashSet<String>>,

    tokens: HashMap<String, CancellationToken>,
}

pub struct LineageHandle {
    limiter: SubagentLimiter,
    agent_id: String,
}

impl LineageHandle {
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
}

impl Drop for LineageHandle {
    fn drop(&mut self) {
        self.limiter.unregister(&self.agent_id);
    }
}

pub enum PermitResult {

    Granted(SubagentPermit),

    Queued,

    Rejected { active: usize, max: usize },
}

#[derive(Debug)]
pub enum QueuedAcquireError {
    Cancelled,
    Rejected { active: usize, max: usize },
    DeadlineExceeded { active: usize, max: usize },
}

pub struct SubagentPermit {
    inner: Option<tokio::sync::OwnedSemaphorePermit>,
    state: Arc<LimiterState>,
}

impl Drop for SubagentPermit {
    fn drop(&mut self) {
        self.state.active.fetch_sub(1, Ordering::SeqCst);
        let Some(permit) = self.inner.take() else {
            return;
        };
        loop {
            let deficit = self.state.deficit.load(Ordering::SeqCst);
            if deficit == 0 {
                return;
            }
            if self
                .state
                .deficit
                .compare_exchange(deficit, deficit - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                permit.forget();
                return;
            }
        }
    }
}

fn clamp_max_concurrent(requested: usize) -> usize {
    let ceiling = crate::constants::system::MAX_CONCURRENT_SUBAGENTS as usize;
    let clamped = requested.clamp(1, ceiling);
    if clamped != requested {
        tracing::warn!(
            target: "agent.subagent_limiter",
            requested,
            effective = clamped,
            ceiling,
            "subagent max_concurrent clamped to the supported ceiling"
        );
    }
    clamped
}

impl SubagentLimiter {
    pub fn new(config: &SubagentLimitConfig) -> Self {
        let max = clamp_max_concurrent(config.max_concurrent);
        Self {
            state: Arc::new(LimiterState {
                semaphore: Arc::new(tokio::sync::Semaphore::new(max)),
                max_concurrent: AtomicUsize::new(max),
                deficit: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
            }),
            queue_excess: config.queue_excess,
            lineage: Arc::new(Mutex::new(LineageTable::default())),
        }
    }

    pub fn set_max_concurrent(&self, max_concurrent: usize) {
        let new_max = clamp_max_concurrent(max_concurrent);
        let old_max = self.state.max_concurrent.swap(new_max, Ordering::SeqCst);
        if new_max > old_max {
            let mut grow = new_max - old_max;
            loop {
                let deficit = self.state.deficit.load(Ordering::SeqCst);
                if deficit == 0 || grow == 0 {
                    break;
                }
                let take = deficit.min(grow);
                if self
                    .state
                    .deficit
                    .compare_exchange(
                        deficit,
                        deficit - take,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    )
                    .is_ok()
                {
                    grow -= take;
                }
            }
            if grow > 0 {
                self.state.semaphore.add_permits(grow);
            }
        } else if new_max < old_max {
            let shrink = old_max - new_max;
            let forgotten = self.state.semaphore.forget_permits(shrink);
            if forgotten < shrink {
                self.state
                    .deficit
                    .fetch_add(shrink - forgotten, Ordering::SeqCst);
            }
        }
    }

    fn grant(&self, permit: tokio::sync::OwnedSemaphorePermit) -> SubagentPermit {
        self.state.active.fetch_add(1, Ordering::SeqCst);
        SubagentPermit {
            inner: Some(permit),
            state: Arc::clone(&self.state),
        }
    }

    pub fn try_acquire(&self) -> PermitResult {
        match Arc::clone(&self.state.semaphore).try_acquire_owned() {
            Ok(permit) => PermitResult::Granted(self.grant(permit)),
            Err(_) => {
                if self.queue_excess {
                    PermitResult::Queued
                } else {
                    PermitResult::Rejected {
                        active: self.active_count(),
                        max: self.max_concurrent(),
                    }
                }
            }
        }
    }

    pub async fn acquire_queued(
        &self,
        cancel: &CancellationToken,
        deadline: Option<std::time::Instant>,
    ) -> Result<SubagentPermit, QueuedAcquireError> {
        match self.try_acquire() {
            PermitResult::Granted(p) => return Ok(p),
            PermitResult::Rejected { active, max } => {
                return Err(QueuedAcquireError::Rejected { active, max });
            }
            PermitResult::Queued => {}
        }
        let acquire = Arc::clone(&self.state.semaphore).acquire_owned();
        tokio::pin!(acquire);
        let deadline_sleep = async {
            match deadline {
                Some(d) => tokio::time::sleep_until(tokio::time::Instant::from_std(d)).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(QueuedAcquireError::Cancelled),
            () = deadline_sleep => Err(QueuedAcquireError::DeadlineExceeded {
                active: self.active_count(),
                max: self.max_concurrent(),
            }),
            acquired = &mut acquire => match acquired {
                Ok(permit) => Ok(self.grant(permit)),
                Err(_) => Err(QueuedAcquireError::Rejected {
                    active: self.active_count(),
                    max: self.max_concurrent(),
                }),
            },
        }
    }

    pub fn active_count(&self) -> usize {
        self.state.active.load(Ordering::SeqCst)
    }

    pub fn max_concurrent(&self) -> usize {
        self.state.max_concurrent.load(Ordering::SeqCst)
    }

    pub fn is_at_capacity(&self) -> bool {
        self.state.semaphore.available_permits() == 0
    }

    pub fn register(
        &self,
        agent_id: impl Into<String>,
        parent_id: Option<String>,
        cancel: CancellationToken,
    ) -> LineageHandle {
        let agent_id = agent_id.into();
        {
            let mut lineage = self.lineage.lock();
            let parent_id = match parent_id {
                Some(p) if p == agent_id || lineage_path_reaches(&lineage, &p, &agent_id) => {
                    tracing::warn!(
                        target: "agent.subagent_limiter",
                        agent_id = %agent_id,
                        parent_id = %p,
                        "rejected lineage parent link that would create a cycle; registering as root"
                    );
                    None
                }
                other => other,
            };
            lineage
                .parents
                .insert(agent_id.clone(), parent_id.clone());
            lineage.tokens.insert(agent_id.clone(), cancel);
            if let Some(parent) = parent_id {
                lineage
                    .children
                    .entry(parent)
                    .or_default()
                    .insert(agent_id.clone());
            }
        }
        LineageHandle {
            limiter: self.clone(),
            agent_id,
        }
    }

    pub fn unregister(&self, agent_id: &str) {
        let mut lineage = self.lineage.lock();
        let parent = lineage.parents.remove(agent_id).flatten();
        lineage.tokens.remove(agent_id);
        if let Some(parent) = parent
            && let Some(set) = lineage.children.get_mut(&parent)
        {
            set.remove(agent_id);
            if set.is_empty() {
                lineage.children.remove(&parent);
            }
        }

        if let Some(orphans) = lineage.children.remove(agent_id) {
            for o in orphans {
                if let Some(slot) = lineage.parents.get_mut(&o) {
                    *slot = None;
                }
            }
        }
    }

    pub fn lineage_size(&self) -> usize {
        self.lineage.lock().parents.len()
    }

    pub fn cancel_descendants(&self, agent_id: &str) -> usize {
        let descendants = self.collect_descendants(agent_id);
        let mut cancelled = 0;
        let lineage = self.lineage.lock();
        for d in &descendants {
            if let Some(token) = lineage.tokens.get(d) {
                token.cancel();
                cancelled += 1;
            }
        }
        if cancelled > 0 {
            debug!(
                target: "agent.subagent_limiter",
                agent_id = %agent_id,
                count = cancelled,
                "subagent_limiter cancelled descendants"
            );
        }
        cancelled
    }

    pub fn cancel_subtree(&self, agent_id: &str) -> usize {
        let mut total = self.cancel_descendants(agent_id);
        let lineage = self.lineage.lock();
        if let Some(token) = lineage.tokens.get(agent_id) {
            token.cancel();
            total += 1;
        }
        total
    }

    pub fn on_overrun(&self, agent_id: &str) -> usize {
        let n = self.cancel_descendants(agent_id);
        if n > 0 {
            debug!(
                target: "agent.subagent_limiter",
                agent_id = %agent_id,
                cascaded = n,
                "on_overrun cascaded cancellation"
            );
        }
        n
    }

    fn collect_descendants(&self, agent_id: &str) -> Vec<String> {
        let lineage = self.lineage.lock();
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(agent_id.to_string());
        let mut out: Vec<String> = Vec::new();
        let mut frontier: Vec<String> = lineage
            .children
            .get(agent_id)
            .map(|c| c.iter().cloned().collect())
            .unwrap_or_default();
        while let Some(next) = frontier.pop() {
            if !visited.insert(next.clone()) {
                continue;
            }
            if let Some(next_children) = lineage.children.get(&next) {
                for c in next_children {
                    if !visited.contains(c) {
                        frontier.push(c.clone());
                    }
                }
            }
            out.push(next);
        }
        out
    }
}

fn lineage_path_reaches(lineage: &LineageTable, from: &str, target: &str) -> bool {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut cursor = from;
    loop {
        if cursor == target {
            return true;
        }
        if !visited.insert(cursor) {
            return false;
        }
        match lineage.parents.get(cursor).and_then(|p| p.as_deref()) {
            Some(parent) => cursor = parent,
            None => return false,
        }
    }
}
