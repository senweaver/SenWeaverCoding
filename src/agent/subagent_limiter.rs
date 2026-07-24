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
    active: Arc<AtomicUsize>,
    max_concurrent: Arc<AtomicUsize>,
    queue_excess: bool,
    lineage: Arc<Mutex<LineageTable>>,
    released: Arc<tokio::sync::Notify>,
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
    active: Arc<AtomicUsize>,
    released: Arc<tokio::sync::Notify>,
}

impl Drop for SubagentPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.released.notify_waiters();
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
            active: Arc::new(AtomicUsize::new(0)),
            max_concurrent: Arc::new(AtomicUsize::new(max)),
            queue_excess: config.queue_excess,
            lineage: Arc::new(Mutex::new(LineageTable::default())),
            released: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn set_max_concurrent(&self, max_concurrent: usize) {
        self.max_concurrent
            .store(clamp_max_concurrent(max_concurrent), Ordering::SeqCst);
        self.released.notify_waiters();
    }

    pub fn try_acquire(&self) -> PermitResult {
        let max = self.max_concurrent.load(Ordering::SeqCst);
        let current = self.active.fetch_add(1, Ordering::SeqCst);
        if current < max {
            PermitResult::Granted(SubagentPermit {
                active: Arc::clone(&self.active),
                released: Arc::clone(&self.released),
            })
        } else {
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.queue_excess {
                PermitResult::Queued
            } else {
                PermitResult::Rejected {
                    active: current,
                    max,
                }
            }
        }
    }

    pub async fn acquire_queued(
        &self,
        cancel: &CancellationToken,
        deadline: Option<std::time::Instant>,
    ) -> Result<SubagentPermit, QueuedAcquireError> {
        loop {
            match self.try_acquire() {
                PermitResult::Granted(p) => return Ok(p),
                PermitResult::Rejected { active, max } => {
                    return Err(QueuedAcquireError::Rejected { active, max });
                }
                PermitResult::Queued => {}
            }
            let notified = self.released.notified();
            if let PermitResult::Granted(p) = self.try_acquire() {
                return Ok(p);
            }
            let sleep_cap = std::time::Duration::from_millis(500);
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(QueuedAcquireError::Cancelled),
                _ = notified => {}
                _ = tokio::time::sleep(sleep_cap) => {}
            }
            if let Some(d) = deadline
                && std::time::Instant::now() > d
            {
                return Err(QueuedAcquireError::DeadlineExceeded {
                    active: self.active_count(),
                    max: self.max_concurrent(),
                });
            }
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    pub fn is_at_capacity(&self) -> bool {
        self.active_count() >= self.max_concurrent.load(Ordering::SeqCst)
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
        let mut out: Vec<String> = Vec::new();
        let mut frontier: Vec<String> = lineage
            .children
            .get(agent_id)
            .map(|c| c.iter().cloned().collect())
            .unwrap_or_default();
        while let Some(next) = frontier.pop() {
            if let Some(next_children) = lineage.children.get(&next) {
                for c in next_children {
                    frontier.push(c.clone());
                }
            }
            out.push(next);
        }
        out
    }
}
