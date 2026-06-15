// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace, warn};

use super::registry::AgentId;
use crate::observability::coordination_metrics::{self, LockAcquireOutcome};

#[derive(Debug, Clone)]
struct LockEntry {

    owner: AgentId,

    acquired_at: Instant,

    ttl: Duration,

    reason: String,
}

impl LockEntry {
    fn is_expired(&self) -> bool {
        self.acquired_at.elapsed() >= self.ttl
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockResult {

    Acquired,

    Held { owner: AgentId },

    AlreadyHeld,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {

    Conflict {
        path: PathBuf,
        holder: AgentId,
        range: Range<usize>,
    },

    Deadlock { cycle: Vec<AgentId> },

    Timeout {
        path: PathBuf,
        range: Range<usize>,
    },

    WorkspaceEscape,
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Conflict {
                path,
                holder,
                range,
            } => write!(
                f,
                "lock conflict on {} [{}..{}]: held by {}",
                path.display(),
                range.start,
                range.end,
                holder
            ),
            LockError::Deadlock { cycle } => {
                write!(f, "deadlock detected: cycle = {}", cycle.join(" ??"))
            }
            LockError::Timeout { path, range } => write!(
                f,
                "lock acquire timed out on {} [{}..{}]",
                path.display(),
                range.start,
                range.end
            ),
            LockError::WorkspaceEscape => write!(f, "lock path escapes workspace"),
        }
    }
}

impl std::error::Error for LockError {}

#[derive(Debug, Clone, Copy)]
pub struct AcquireOpts {

    pub wait_timeout: Duration,

    pub deadlock_detect: bool,

    pub ttl: Option<Duration>,
}

impl Default for AcquireOpts {
    fn default() -> Self {
        Self {
            wait_timeout: Duration::from_millis(0),
            deadlock_detect: true,
            ttl: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegionRequest {
    pub path: PathBuf,
    pub range: Range<usize>,

    pub exclusive: bool,
}

#[derive(Debug, Clone)]
struct FileRegionLockEntry {
    id: u64,
    range: Range<usize>,
    holder: AgentId,
    exclusive: bool,
    acquired_at: Instant,
    ttl: Duration,
}

impl FileRegionLockEntry {
    fn is_expired(&self) -> bool {
        self.acquired_at.elapsed() >= self.ttl
    }
    fn overlaps(&self, range: &Range<usize>) -> bool {
        self.range.start < range.end && range.start < self.range.end
    }
}

#[must_use = "RegionLockToken releases on drop; store it or the lock leaks"]
pub struct RegionLockToken {
    manager: Arc<LockManager>,

    state: Option<RegionTokenState>,
}

#[derive(Debug, Clone)]
struct RegionTokenState {
    id: u64,
    path: PathBuf,
    range: Range<usize>,
    holder: AgentId,
    exclusive: bool,
}

impl RegionLockToken {
    pub fn path(&self) -> Option<&Path> {
        self.state.as_ref().map(|s| s.path.as_path())
    }
    pub fn range(&self) -> Option<Range<usize>> {
        self.state.as_ref().map(|s| s.range.clone())
    }
    pub fn is_exclusive(&self) -> bool {
        self.state.as_ref().map(|s| s.exclusive).unwrap_or(false)
    }
    pub fn holder(&self) -> Option<&str> {
        self.state.as_ref().map(|s| s.holder.as_str())
    }

    pub fn release(&mut self) {
        if let Some(state) = self.state.take() {
            self.manager.release_region_entry(state.id, &state.path);
        }
    }
}

impl std::fmt::Debug for RegionLockToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegionLockToken")
            .field("state", &self.state)
            .finish()
    }
}

impl Drop for RegionLockToken {
    fn drop(&mut self) {
        self.release();
    }
}

#[must_use = "RegionLockTokens releases on drop"]
pub struct RegionLockTokens {
    tokens: Vec<RegionLockToken>,
}

impl RegionLockTokens {
    pub fn len(&self) -> usize {
        self.tokens.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, RegionLockToken> {
        self.tokens.iter()
    }
    pub fn release_all(&mut self) {
        while let Some(mut tok) = self.tokens.pop() {
            tok.release();
        }
    }
}

impl<'a> IntoIterator for &'a RegionLockTokens {
    type Item = &'a RegionLockToken;
    type IntoIter = std::slice::Iter<'a, RegionLockToken>;
    fn into_iter(self) -> Self::IntoIter {
        self.tokens.iter()
    }
}

impl std::fmt::Debug for RegionLockTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegionLockTokens")
            .field("tokens", &self.tokens.len())
            .finish()
    }
}

#[must_use = "BufferLock releases the lock on drop; store it or the lock leaks"]
pub struct BufferLock {

    manager: Arc<LockManager>,

    resource: String,

    agent_id: AgentId,
}

impl Drop for BufferLock {
    fn drop(&mut self) {
        let released = self.manager.locks.write().remove(&self.resource);
        if released.is_some() {
            debug!(resource = %self.resource, agent = %self.agent_id, "BufferLock dropped ??lock released");
        }
    }
}

impl std::fmt::Debug for BufferLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferLock")
            .field("resource", &self.resource)
            .field("agent_id", &self.agent_id)
            .finish()
    }
}

impl BufferLock {

    pub fn no_op(resource: String) -> Self {
        Self {
            manager: Arc::new(LockManager::default()),
            resource,
            agent_id: String::new(),
        }
    }
}

pub struct LockManager {
    locks: RwLock<HashMap<String, LockEntry>>,
    default_ttl: Duration,

    file_regions: RwLock<HashMap<PathBuf, Vec<FileRegionLockEntry>>>,

    wait_graph: RwLock<HashMap<AgentId, HashSet<AgentId>>>,

    next_token_id: AtomicU64,

    region_release_mutex: parking_lot::Mutex<()>,
    region_release_cv: parking_lot::Condvar,
}

impl LockManager {

    pub fn new(default_ttl: Duration) -> Self {
        Self {
            locks: RwLock::new(HashMap::new()),
            default_ttl,
            file_regions: RwLock::new(HashMap::new()),
            wait_graph: RwLock::new(HashMap::new()),
            next_token_id: AtomicU64::new(1),
            region_release_mutex: parking_lot::Mutex::new(()),
            region_release_cv: parking_lot::Condvar::new(),
        }
    }

    fn wait_for_region_release(&self, deadline: Instant) {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let cap = Duration::from_millis(50);
        let wait_for = deadline.saturating_duration_since(now).min(cap);
        let mut guard = self.region_release_mutex.lock();
        let _ = self.region_release_cv.wait_for(&mut guard, wait_for);
    }

    pub fn acquire(&self, resource: &str, agent_id: &str, reason: &str) -> LockResult {
        self.acquire_with_ttl(resource, agent_id, reason, self.default_ttl)
    }

    pub fn acquire_with_ttl(
        &self,
        resource: &str,
        agent_id: &str,
        reason: &str,
        ttl: Duration,
    ) -> LockResult {
        let denied_owner;
        {
            let mut locks = self.locks.write();

            if let Some(existing) = locks.get(resource) {
                if existing.is_expired() {
                    debug!(
                        resource = %resource,
                        expired_owner = %existing.owner,
                        "Lock expired, allowing acquisition"
                    );
                    denied_owner = None;
                } else if existing.owner == agent_id {
                    return LockResult::AlreadyHeld;
                } else {
                    denied_owner = Some(existing.owner.clone());
                }
            } else {
                denied_owner = None;
            }

            if denied_owner.is_none() {
                locks.insert(
                    resource.to_string(),
                    LockEntry {
                        owner: agent_id.to_string(),
                        acquired_at: Instant::now(),
                        ttl,
                        reason: reason.to_string(),
                    },
                );
            }
        }

        match denied_owner {
            Some(owner) => {
                crate::event_bus::integration::publish_coordination_now(
                    agent_id,
                    crate::event_bus::types::CoordinationAction::LockDenied,
                    resource,
                    Some(serde_json::json!({ "owner": owner })),
                );
                LockResult::Held { owner }
            }
            None => {
                debug!(resource = %resource, agent = %agent_id, "Lock acquired");
                crate::event_bus::integration::publish_coordination_now(
                    agent_id,
                    crate::event_bus::types::CoordinationAction::LockGranted,
                    resource,
                    None,
                );
                LockResult::Acquired
            }
        }
    }

    pub fn release(&self, resource: &str, agent_id: &str) -> bool {
        let removed;
        {
            let mut locks = self.locks.write();
            if let Some(entry) = locks.get(resource) {
                if entry.owner == agent_id || entry.is_expired() {
                    locks.remove(resource);
                    debug!(resource = %resource, agent = %agent_id, "Lock released");
                    removed = true;
                } else {
                    warn!(
                        resource = %resource,
                        agent = %agent_id,
                        owner = %entry.owner,
                        "Lock release denied: not the owner"
                    );
                    return false;
                }
            } else {
                return true;
            }
        }
        if removed {
            crate::event_bus::integration::publish_coordination_now(
                agent_id,
                crate::event_bus::types::CoordinationAction::LockRelease,
                resource,
                None,
            );
        }
        true
    }

    pub fn force_release(&self, resource: &str) -> bool {
        self.locks.write().remove(resource).is_some()
    }

    pub fn is_locked(&self, resource: &str) -> bool {
        let locks = self.locks.read();
        locks
            .get(resource)
            .map(|e| !e.is_expired())
            .unwrap_or(false)
    }

    pub fn lock_owner(&self, resource: &str) -> Option<AgentId> {
        let locks = self.locks.read();
        locks
            .get(resource)
            .filter(|e| !e.is_expired())
            .map(|e| e.owner.clone())
    }

    pub fn release_all_for_agent(&self, agent_id: &str) -> usize {
        let mut locks = self.locks.write();
        let before = locks.len();
        locks.retain(|_, entry| entry.owner != agent_id);
        let released = before - locks.len();
        if released > 0 {
            debug!(agent = %agent_id, count = released, "Released all locks for agent");
        }
        released
    }

    pub fn evict_expired(&self) -> usize {
        let mut locks = self.locks.write();
        let before = locks.len();
        locks.retain(|_, entry| !entry.is_expired());
        before - locks.len()
    }

    pub fn all_locks(&self) -> Vec<(String, AgentId, String)> {
        self.locks
            .read()
            .iter()
            .filter(|(_, e)| !e.is_expired())
            .map(|(k, e)| (k.clone(), e.owner.clone(), e.reason.clone()))
            .collect()
    }

    pub fn lock_count(&self) -> usize {
        self.locks
            .read()
            .values()
            .filter(|e| !e.is_expired())
            .count()
    }

    pub fn buffer_lock(
        self: &Arc<Self>,
        buffer_id: &str,
        agent_id: &str,
    ) -> Result<BufferLock, LockResult> {
        let result = self.acquire(buffer_id, agent_id, "editing buffer");
        match result {
            LockResult::Acquired | LockResult::AlreadyHeld => Ok(BufferLock {
                manager: Arc::clone(self),
                resource: buffer_id.to_string(),
                agent_id: agent_id.to_string(),
            }),
            LockResult::Held { owner } => Err(LockResult::Held { owner }),
        }
    }

    pub fn acquire_region(
        self: &Arc<Self>,
        path: &Path,
        range: Range<usize>,
        agent_id: &str,
        opts: AcquireOpts,
    ) -> Result<RegionLockToken, LockError> {
        if path.as_os_str().is_empty() {
            coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Conflict);
            return Err(LockError::WorkspaceEscape);
        }
        let path_buf = path.to_path_buf();
        let ttl = opts.ttl.unwrap_or(self.default_ttl);
        let deadline = if opts.wait_timeout.is_zero() {
            None
        } else {
            Some(Instant::now() + opts.wait_timeout)
        };

        loop {

            let conflict_entry: Option<FileRegionLockEntry> = {
                let mut regions = self.file_regions.write();
                let entries = regions.entry(path_buf.clone()).or_default();
                entries.retain(|e| !e.is_expired());

                let mut hit = None;
                for existing in entries.iter() {
                    if existing.holder == agent_id {
                        continue;
                    }
                    if !existing.overlaps(&range) {
                        continue;
                    }

                    hit = Some(existing.clone());
                    break;
                }
                if hit.is_none() {
                    let id = self.next_token_id.fetch_add(1, Ordering::Relaxed);
                    entries.push(FileRegionLockEntry {
                        id,
                        range: range.clone(),
                        holder: agent_id.to_string(),
                        exclusive: true,
                        acquired_at: Instant::now(),
                        ttl,
                    });
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Ok);
                    debug!(
                        path = %path_buf.display(),
                        start = range.start,
                        end = range.end,
                        agent = %agent_id,
                        "Region lock acquired (exclusive)"
                    );
                    return Ok(RegionLockToken {
                        manager: Arc::clone(self),
                        state: Some(RegionTokenState {
                            id,
                            path: path_buf,
                            range,
                            holder: agent_id.to_string(),
                            exclusive: true,
                        }),
                    });
                }
                hit
            };

            let Some(blocker) = conflict_entry else {
                continue;
            };

            if opts.deadlock_detect {

                self.add_wait_edge(agent_id, &blocker.holder);
                if let Some(cycle) = self.detect_cycle(agent_id) {
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Deadlock);
                    coordination_metrics::incr_lockmgr_deadlock_detected();
                    warn!(
                        agent = %agent_id,
                        cycle = ?cycle,
                        "Deadlock detected during acquire_region"
                    );
                    return Err(LockError::Deadlock { cycle });
                }
            }

            match deadline {
                None => {
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Conflict);
                    return Err(LockError::Conflict {
                        path: path_buf,
                        holder: blocker.holder,
                        range: blocker.range,
                    });
                }
                Some(t) if Instant::now() >= t => {
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Timeout);
                    return Err(LockError::Timeout {
                        path: path_buf,
                        range,
                    });
                }
                Some(t) => {
                    self.wait_for_region_release(t);
                }
            }
        }
    }

    pub fn acquire_region_shared(
        self: &Arc<Self>,
        path: &Path,
        range: Range<usize>,
        agent_id: &str,
        opts: AcquireOpts,
    ) -> Result<RegionLockToken, LockError> {
        if path.as_os_str().is_empty() {
            coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Conflict);
            return Err(LockError::WorkspaceEscape);
        }
        let path_buf = path.to_path_buf();
        let ttl = opts.ttl.unwrap_or(self.default_ttl);
        let deadline = if opts.wait_timeout.is_zero() {
            None
        } else {
            Some(Instant::now() + opts.wait_timeout)
        };

        loop {
            let conflict_entry: Option<FileRegionLockEntry> = {
                let mut regions = self.file_regions.write();
                let entries = regions.entry(path_buf.clone()).or_default();
                entries.retain(|e| !e.is_expired());

                let mut hit = None;
                for existing in entries.iter() {
                    if existing.holder == agent_id {
                        continue;
                    }
                    if !existing.overlaps(&range) {
                        continue;
                    }

                    if existing.exclusive {
                        hit = Some(existing.clone());
                        break;
                    }
                }
                if hit.is_none() {
                    let id = self.next_token_id.fetch_add(1, Ordering::Relaxed);
                    entries.push(FileRegionLockEntry {
                        id,
                        range: range.clone(),
                        holder: agent_id.to_string(),
                        exclusive: false,
                        acquired_at: Instant::now(),
                        ttl,
                    });
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Ok);
                    return Ok(RegionLockToken {
                        manager: Arc::clone(self),
                        state: Some(RegionTokenState {
                            id,
                            path: path_buf,
                            range,
                            holder: agent_id.to_string(),
                            exclusive: false,
                        }),
                    });
                }
                hit
            };

            let Some(blocker) = conflict_entry else {
                continue;
            };

            if opts.deadlock_detect {
                self.add_wait_edge(agent_id, &blocker.holder);
                if let Some(cycle) = self.detect_cycle(agent_id) {
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Deadlock);
                    coordination_metrics::incr_lockmgr_deadlock_detected();
                    return Err(LockError::Deadlock { cycle });
                }
            }

            match deadline {
                None => {
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Conflict);
                    return Err(LockError::Conflict {
                        path: path_buf,
                        holder: blocker.holder,
                        range: blocker.range,
                    });
                }
                Some(t) if Instant::now() >= t => {
                    self.clear_wait_edges(agent_id);
                    coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Timeout);
                    return Err(LockError::Timeout {
                        path: path_buf,
                        range,
                    });
                }
                Some(t) => {
                    self.wait_for_region_release(t);
                }
            }
        }
    }

    pub fn acquire_multi(
        self: &Arc<Self>,
        specs: &[RegionRequest],
        agent_id: &str,
        opts: AcquireOpts,
    ) -> Result<RegionLockTokens, LockError> {
        let mut acquired: Vec<RegionLockToken> = Vec::with_capacity(specs.len());
        for spec in specs {
            let token_result = if spec.exclusive {
                self.acquire_region(&spec.path, spec.range.clone(), agent_id, opts)
            } else {
                self.acquire_region_shared(&spec.path, spec.range.clone(), agent_id, opts)
            };
            match token_result {
                Ok(tok) => acquired.push(tok),
                Err(err) => {

                    drop(acquired);
                    return Err(err);
                }
            }
        }
        Ok(RegionLockTokens { tokens: acquired })
    }

    pub fn upgrade_to_exclusive(
        self: &Arc<Self>,
        token: &mut RegionLockToken,
    ) -> Result<(), LockError> {
        let state = match token.state.as_ref() {
            Some(s) => s.clone(),
            None => {
                return Err(LockError::Conflict {
                    path: PathBuf::new(),
                    holder: String::new(),
                    range: 0..0,
                });
            }
        };
        if state.exclusive {
            return Ok(());
        }

        let mut regions = self.file_regions.write();
        let entries = regions.entry(state.path.clone()).or_default();
        for existing in entries.iter() {
            if existing.id == state.id {
                continue;
            }
            if existing.holder == state.holder {
                continue;
            }
            if existing.overlaps(&state.range) {
                coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Conflict);
                return Err(LockError::Conflict {
                    path: state.path.clone(),
                    holder: existing.holder.clone(),
                    range: existing.range.clone(),
                });
            }
        }
        for existing in entries.iter_mut() {
            if existing.id == state.id {
                existing.exclusive = true;
            }
        }
        if let Some(s) = token.state.as_mut() {
            s.exclusive = true;
        }
        coordination_metrics::incr_lockmgr_acquire(LockAcquireOutcome::Ok);
        Ok(())
    }

    pub fn downgrade_to_shared(
        self: &Arc<Self>,
        token: &mut RegionLockToken,
    ) -> Result<(), LockError> {
        let state = match token.state.as_ref() {
            Some(s) => s.clone(),
            None => {
                return Err(LockError::Conflict {
                    path: PathBuf::new(),
                    holder: String::new(),
                    range: 0..0,
                });
            }
        };
        if !state.exclusive {
            return Ok(());
        }
        let mut regions = self.file_regions.write();
        if let Some(entries) = regions.get_mut(&state.path) {
            for existing in entries.iter_mut() {
                if existing.id == state.id {
                    existing.exclusive = false;
                }
            }
        }
        if let Some(s) = token.state.as_mut() {
            s.exclusive = false;
        }
        Ok(())
    }

    fn release_region_entry(&self, id: u64, path: &Path) {
        {
            let mut regions = self.file_regions.write();
            if let Some(entries) = regions.get_mut(path) {
                entries.retain(|e| e.id != id);
                if entries.is_empty() {
                    regions.remove(path);
                }
            }
        }
        self.region_release_cv.notify_all();
        coordination_metrics::incr_lockmgr_release();
    }

    fn add_wait_edge(&self, requester: &str, blocker: &str) {
        let mut g = self.wait_graph.write();
        g.entry(requester.to_string())
            .or_default()
            .insert(blocker.to_string());
    }

    fn clear_wait_edges(&self, requester: &str) {
        let mut g = self.wait_graph.write();
        g.remove(requester);
    }

    fn detect_cycle(&self, start: &str) -> Option<Vec<AgentId>> {
        let g = self.wait_graph.read();
        let mut stack: Vec<(AgentId, Vec<AgentId>)> = vec![(start.to_string(), vec![])];
        let mut visited: HashSet<AgentId> = HashSet::new();
        while let Some((node, path)) = stack.pop() {
            if !visited.insert(node.clone()) {
                continue;
            }
            let mut path = path;
            path.push(node.clone());
            if let Some(out_edges) = g.get(&node) {
                for next in out_edges.iter() {
                    if next == start {
                        let mut cycle = path.clone();
                        cycle.push(start.to_string());
                        return Some(cycle);
                    }
                    if !visited.contains(next) {
                        stack.push((next.clone(), path.clone()));
                    }
                }
            }
        }
        None
    }

    pub fn region_lock_count(&self) -> usize {
        self.file_regions.read().values().map(Vec::len).sum()
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

#[derive(Debug, Clone)]
struct BarrierState {

    expected: HashSet<AgentId>,

    arrived: HashSet<AgentId>,

    created_at: Instant,

    timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarrierResult {

    Waiting { arrived: usize, expected: usize },

    Released,

    TimedOut,

    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BarrierError {

    #[error("barrier '{name}' rejected: expected_agents must not be empty")]
    EmptyExpected { name: String },
}

pub struct BarrierManager {
    barriers: RwLock<HashMap<String, BarrierState>>,
}

impl BarrierManager {
    pub fn new() -> Self {
        Self {
            barriers: RwLock::new(HashMap::new()),
        }
    }

    pub fn create_barrier(
        &self,
        name: &str,
        expected_agents: HashSet<AgentId>,
        timeout: Duration,
    ) -> Result<(), BarrierError> {
        if expected_agents.is_empty() {
            return Err(BarrierError::EmptyExpected {
                name: name.to_string(),
            });
        }
        let mut barriers = self.barriers.write();
        barriers.insert(
            name.to_string(),
            BarrierState {
                expected: expected_agents.clone(),
                arrived: HashSet::new(),
                created_at: Instant::now(),
                timeout,
            },
        );
        info!(
            barrier = %name,
            expected = expected_agents.len(),
            "Barrier created"
        );
        Ok(())
    }

    pub fn arrive(&self, barrier_name: &str, agent_id: &str) -> BarrierResult {
        let result = {
            let mut barriers = self.barriers.write();
            let barrier = match barriers.get_mut(barrier_name) {
                Some(b) => b,
                None => return BarrierResult::NotFound,
            };

            if barrier.created_at.elapsed() >= barrier.timeout {
                barriers.remove(barrier_name);
                return BarrierResult::TimedOut;
            }

            barrier.arrived.insert(agent_id.to_string());
            debug!(
                barrier = %barrier_name,
                agent = %agent_id,
                arrived = barrier.arrived.len(),
                expected = barrier.expected.len(),
                "Agent arrived at barrier"
            );

            if barrier.arrived.is_superset(&barrier.expected) {
                barriers.remove(barrier_name);
                info!(barrier = %barrier_name, "Barrier released ??all agents arrived");
                BarrierResult::Released
            } else {
                BarrierResult::Waiting {
                    arrived: barrier.arrived.len(),
                    expected: barrier.expected.len(),
                }
            }
        };

        match &result {
            BarrierResult::Released => {
                crate::event_bus::integration::publish_coordination_now(
                    agent_id,
                    crate::event_bus::types::CoordinationAction::BarrierRelease,
                    barrier_name,
                    None,
                );
            }
            BarrierResult::Waiting { arrived, expected } => {
                crate::event_bus::integration::publish_coordination_now(
                    agent_id,
                    crate::event_bus::types::CoordinationAction::BarrierReady,
                    barrier_name,
                    Some(serde_json::json!({ "arrived": arrived, "expected": expected })),
                );
            }
            _ => {}
        }
        result
    }

    pub fn status(&self, barrier_name: &str) -> Option<(usize, usize)> {
        let barriers = self.barriers.read();
        barriers
            .get(barrier_name)
            .map(|b| (b.arrived.len(), b.expected.len()))
    }

    pub fn remove(&self, barrier_name: &str) -> bool {
        self.barriers.write().remove(barrier_name).is_some()
    }

    pub fn count(&self) -> usize {
        self.barriers.read().len()
    }

    pub fn evict_expired(&self) -> usize {
        let mut barriers = self.barriers.write();
        let before = barriers.len();
        barriers.retain(|_, b| b.created_at.elapsed() < b.timeout);
        before - barriers.len()
    }
}

impl Default for BarrierManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub agent_id: AgentId,
    pub value: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct VotingSession {

    eligible: HashSet<AgentId>,

    votes: Vec<Vote>,

    created_at: Instant,

    timeout: Duration,

    majority: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VotingResult {

    Recorded {
        votes_cast: usize,
        votes_needed: usize,
    },

    Consensus { winning_value: String, votes: usize },

    NoConsensus { tally: HashMap<String, usize> },

    NotFound,

    AlreadyVoted,

    TimedOut,
}

pub struct VotingManager {
    sessions: RwLock<HashMap<String, VotingSession>>,
}

impl VotingManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn start_session(
        &self,
        session_id: &str,
        topic: &str,
        initiator: &str,
        eligible: HashSet<AgentId>,
        timeout: Duration,
        majority: f64,
    ) {
        let mut sessions = self.sessions.write();
        sessions.insert(
            session_id.to_string(),
            VotingSession {
                eligible: eligible.clone(),
                votes: Vec::new(),
                created_at: Instant::now(),
                timeout,
                majority: majority.clamp(0.0, 1.0),
            },
        );
        info!(
            session = %session_id,
            topic = %topic,
            eligible = eligible.len(),
            "Voting session started"
        );
        crate::event_bus::integration::publish_coordination_now(
            initiator,
            crate::event_bus::types::CoordinationAction::Propose,
            topic,
            Some(serde_json::json!({ "session_id": session_id })),
        );
    }

    pub fn cast_vote(&self, session_id: &str, agent_id: &str, value: &str) -> VotingResult {
        let result = 'voting: {
            let mut sessions = self.sessions.write();
            let session = match sessions.get_mut(session_id) {
                Some(s) => s,
                None => return VotingResult::NotFound,
            };

            if session.created_at.elapsed() >= session.timeout {
                let _tally = Self::compute_tally(&session.votes);
                sessions.remove(session_id);
                return VotingResult::TimedOut;
            }

            if session.votes.iter().any(|v| v.agent_id == agent_id) {
                return VotingResult::AlreadyVoted;
            }

            session.votes.push(Vote {
                agent_id: agent_id.to_string(),
                value: value.to_string(),
                timestamp: Utc::now(),
            });

            debug!(
                session = %session_id,
                agent = %agent_id,
                value = %value,
                "Vote cast"
            );

            let eligible_count = session.eligible.len();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let needed = (eligible_count as f64 * session.majority).ceil() as usize;
            let tally = Self::compute_tally(&session.votes);

            for (val, count) in &tally {
                if *count >= needed {
                    let winning = val.clone();
                    let votes = *count;
                    sessions.remove(session_id);
                    info!(
                        session = %session_id,
                        value = %winning,
                        "Consensus reached"
                    );
                    break 'voting VotingResult::Consensus {
                        winning_value: winning,
                        votes,
                    };
                }
            }

            if session.votes.len() >= eligible_count {
                let tally_clone = tally.clone();
                sessions.remove(session_id);
                break 'voting VotingResult::NoConsensus { tally: tally_clone };
            }

            VotingResult::Recorded {
                votes_cast: session.votes.len(),
                votes_needed: needed,
            }
        };

        match &result {
            VotingResult::Recorded {
                votes_cast,
                votes_needed,
            } => {
                crate::event_bus::integration::publish_coordination_now(
                    agent_id,
                    crate::event_bus::types::CoordinationAction::Vote,
                    session_id,
                    Some(serde_json::json!({
                        "value": value,
                        "votes_cast": votes_cast,
                        "votes_needed": votes_needed,
                    })),
                );
            }
            VotingResult::Consensus {
                winning_value,
                votes,
            } => {
                crate::event_bus::integration::publish_coordination_now(
                    agent_id,
                    crate::event_bus::types::CoordinationAction::Commit,
                    session_id,
                    Some(serde_json::json!({
                        "winning_value": winning_value,
                        "votes": votes,
                    })),
                );
            }
            _ => {}
        }
        result
    }

    pub fn tally(&self, session_id: &str) -> Option<HashMap<String, usize>> {
        let sessions = self.sessions.read();
        sessions
            .get(session_id)
            .map(|s| Self::compute_tally(&s.votes))
    }

    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }

    pub fn evict_expired(&self) -> usize {
        let mut sessions = self.sessions.write();
        let before = sessions.len();
        sessions.retain(|_, s| s.created_at.elapsed() < s.timeout);
        before - sessions.len()
    }

    fn compute_tally(votes: &[Vote]) -> HashMap<String, usize> {
        let mut tally = HashMap::new();
        for vote in votes {
            *tally.entry(vote.value.clone()).or_insert(0) += 1;
        }
        tally
    }
}

impl Default for VotingManager {
    fn default() -> Self {
        Self::new()
    }
}

const COORD_REQUEST_BARRIER_TIMEOUT: Duration = Duration::from_secs(3600);
const COORD_TASK_BARRIER_TIMEOUT: Duration = Duration::from_secs(3600);
const COORD_OUTCOME_VOTE_TIMEOUT: Duration = Duration::from_secs(120);
const COORD_RATIFY_MAJORITY: f64 = 1.0;
const COORD_TASK_COMPLETION_TOKEN: &str = "completion";

pub struct Coordinator {
    pub locks: Arc<LockManager>,
    pub barriers: BarrierManager,
    pub voting: VotingManager,

    event_subscriber_started: AtomicBool,
}

impl Coordinator {
    pub fn new() -> Self {
        Self {
            locks: Arc::new(LockManager::default()),
            barriers: BarrierManager::new(),
            voting: VotingManager::new(),
            event_subscriber_started: AtomicBool::new(false),
        }
    }

    pub fn with_lock_ttl(lock_ttl: Duration) -> Self {
        Self {
            locks: Arc::new(LockManager::new(lock_ttl)),
            barriers: BarrierManager::new(),
            voting: VotingManager::new(),
            event_subscriber_started: AtomicBool::new(false),
        }
    }

    pub fn locks_arc(&self) -> Arc<LockManager> {
        Arc::clone(&self.locks)
    }

    pub fn maintenance(&self) -> (usize, usize, usize) {
        let locks = self.locks.evict_expired();
        let barriers = self.barriers.evict_expired();
        let voting = self.voting.evict_expired();
        (locks, barriers, voting)
    }

    pub fn spawn_event_subscriber(self: Arc<Self>) {
        if self.event_subscriber_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let coordinator = self;
        crate::runtime::spawn_supervised(
            "agent.coordinator.event_subscriber",
            async move {
                let mut rx = loop {
                    match crate::event_bus::integration::global_bus() {
                        Some(bus) => break bus.subscribe_all(),
                        None => tokio::time::sleep(Duration::from_secs(5)).await,
                    }
                };

                loop {
                    match rx.recv().await {
                        Ok(event) => coordinator.on_coordination_event(&event),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(skipped, "coordinator event subscriber lagged; continuing");
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            match crate::event_bus::integration::global_bus() {
                                Some(bus) => {
                                    rx = bus.subscribe_all();
                                    tokio::time::sleep(Duration::from_secs(1)).await;
                                    continue;
                                }
                                None => break,
                            }
                        }
                    }
                }
            },
        );
    }

    fn on_coordination_event(&self, event: &crate::event_bus::types::Event) {
        use crate::event_bus::types::{EventPayload, EventTarget, TaskDelegationAction};

        match &event.payload {
            EventPayload::AgentRequest {
                request_id,
                capability,
                ..
            } => {
                if let EventTarget::Agent(agent) = &event.target {
                    let barrier = format!("request:{request_id}");
                    let mut expected = HashSet::new();
                    expected.insert(agent.clone());
                    match self.barriers.create_barrier(
                        &barrier,
                        expected,
                        COORD_REQUEST_BARRIER_TIMEOUT,
                    ) {
                        Ok(()) => debug!(
                            request_id = %request_id,
                            agent = %agent,
                            capability = %capability,
                            "coordinator: opened request barrier"
                        ),
                        Err(e) => warn!(
                            request_id = %request_id,
                            error = %e,
                            "coordinator: failed to open request barrier"
                        ),
                    }
                }
            }
            EventPayload::AgentResponse {
                request_id,
                success,
                ..
            } => {
                let barrier = format!("request:{request_id}");
                let agent = event.source.clone();
                match self.barriers.arrive(&barrier, &agent) {
                    BarrierResult::Released => {
                        debug!(
                            request_id = %request_id,
                            agent = %agent,
                            "coordinator: request barrier released"
                        );
                        self.ratify_outcome(
                            &format!("request-outcome:{request_id}"),
                            &agent,
                            *success,
                        );
                    }
                    BarrierResult::Waiting { arrived, expected } => debug!(
                        request_id = %request_id,
                        arrived,
                        expected,
                        "coordinator: request barrier waiting"
                    ),
                    BarrierResult::NotFound => {
                        self.ratify_outcome(
                            &format!("request-outcome:{request_id}"),
                            &agent,
                            *success,
                        );
                    }
                    BarrierResult::TimedOut => debug!(
                        request_id = %request_id,
                        "coordinator: request barrier timed out"
                    ),
                }
            }
            EventPayload::TaskDelegation {
                task_id, action, ..
            } => {
                let barrier = format!("task:{task_id}");
                match action {
                    TaskDelegationAction::Assigned => {
                        let mut expected = HashSet::new();
                        expected.insert(COORD_TASK_COMPLETION_TOKEN.to_string());
                        match self.barriers.create_barrier(
                            &barrier,
                            expected,
                            COORD_TASK_BARRIER_TIMEOUT,
                        ) {
                            Ok(()) => debug!(task_id = %task_id, "coordinator: opened task barrier"),
                            Err(e) => warn!(
                                task_id = %task_id,
                                error = %e,
                                "coordinator: failed to open task barrier"
                            ),
                        }
                    }
                    TaskDelegationAction::Completed | TaskDelegationAction::Failed => {
                        let succeeded = matches!(action, TaskDelegationAction::Completed);
                        match self.barriers.arrive(&barrier, COORD_TASK_COMPLETION_TOKEN) {
                            BarrierResult::Released => {
                                debug!(
                                    task_id = %task_id,
                                    agent = %event.source,
                                    "coordinator: task barrier released"
                                );
                                self.ratify_outcome(
                                    &format!("task-outcome:{task_id}"),
                                    &event.source,
                                    succeeded,
                                );
                            }
                            BarrierResult::NotFound => {
                                self.ratify_outcome(
                                    &format!("task-outcome:{task_id}"),
                                    &event.source,
                                    succeeded,
                                );
                            }
                            other => debug!(
                                task_id = %task_id,
                                ?other,
                                "coordinator: task barrier state"
                            ),
                        }
                    }
                    _ => {}
                }
            }
            EventPayload::Coordination { action, topic, .. } => {
                trace!(
                    ?action,
                    %topic,
                    source = %event.source,
                    "coordinator observed coordination event"
                );
            }
            _ => {}
        }
    }

    fn ratify_outcome(&self, session_id: &str, agent: &str, success: bool) {
        let mut eligible = HashSet::new();
        eligible.insert(agent.to_string());
        self.voting.start_session(
            session_id,
            "delegation outcome ratification",
            agent,
            eligible,
            COORD_OUTCOME_VOTE_TIMEOUT,
            COORD_RATIFY_MAJORITY,
        );
        let value = if success { "success" } else { "failure" };
        match self.voting.cast_vote(session_id, agent, value) {
            VotingResult::Consensus { winning_value, .. } => info!(
                session = %session_id,
                agent = %agent,
                outcome = %winning_value,
                "coordinator: delegation outcome ratified"
            ),
            other => debug!(
                session = %session_id,
                ?other,
                "coordinator: delegation outcome vote state"
            ),
        }
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct CoordinatorHandle {
    inner: Arc<Coordinator>,
}

impl CoordinatorHandle {
    pub fn new(coordinator: Coordinator) -> Self {
        Self {
            inner: Arc::new(coordinator),
        }
    }

    pub fn inner(&self) -> &Coordinator {
        &self.inner
    }

    pub fn locks(&self) -> &LockManager {
        &self.inner.locks
    }

    pub fn locks_arc(&self) -> Arc<LockManager> {
        self.inner.locks_arc()
    }

    pub fn barriers(&self) -> &BarrierManager {
        &self.inner.barriers
    }

    pub fn voting(&self) -> &VotingManager {
        &self.inner.voting
    }

    pub fn maintenance(&self) -> (usize, usize, usize) {
        self.inner.maintenance()
    }

    pub fn spawn_event_subscriber(&self) {
        Coordinator::spawn_event_subscriber(Arc::clone(&self.inner));
    }
}

impl From<Coordinator> for CoordinatorHandle {
    fn from(c: Coordinator) -> Self {
        Self::new(c)
    }
}
