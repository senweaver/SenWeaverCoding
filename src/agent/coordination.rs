// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Coordination Protocol — consensus, resource locking, and barrier synchronization.
//!
//! Provides primitives for multi-agent coordination:
//! - **Distributed locks** with ownership tracking and expiration
//! - **Barrier synchronization** for phased multi-agent workflows
//! - **Voting / consensus** for collaborative decision making

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::registry::AgentId;

// ── Distributed Locks ───────────────────────────────────────────

/// A distributed lock entry.
#[derive(Debug, Clone)]
struct LockEntry {
    /// Which agent holds the lock.
    owner: AgentId,
    /// When the lock was acquired.
    acquired_at: Instant,
    /// Maximum hold duration (auto-release after this).
    ttl: Duration,
    /// Human-readable reason for holding the lock.
    reason: String,
}

impl LockEntry {
    fn is_expired(&self) -> bool {
        self.acquired_at.elapsed() >= self.ttl
    }
}

/// Result of a lock acquisition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockResult {
    /// Lock acquired successfully.
    Acquired,
    /// Lock is held by another agent.
    Held { owner: AgentId },
    /// Lock was already held by the requesting agent (re-entrant).
    AlreadyHeld,
}

/// Distributed lock manager for resource coordination.
pub struct LockManager {
    locks: RwLock<HashMap<String, LockEntry>>,
    default_ttl: Duration,
}

impl LockManager {
    /// Create a new lock manager with a default TTL.
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            locks: RwLock::new(HashMap::new()),
            default_ttl,
        }
    }

    /// Try to acquire a lock on a resource.
    pub fn acquire(&self, resource: &str, agent_id: &str, reason: &str) -> LockResult {
        self.acquire_with_ttl(resource, agent_id, reason, self.default_ttl)
    }

    /// Try to acquire a lock with a custom TTL.
    pub fn acquire_with_ttl(
        &self,
        resource: &str,
        agent_id: &str,
        reason: &str,
        ttl: Duration,
    ) -> LockResult {
        let mut locks = self.locks.write();

        // Check for existing lock
        if let Some(existing) = locks.get(resource) {
            if existing.is_expired() {
                debug!(
                    resource = %resource,
                    expired_owner = %existing.owner,
                    "Lock expired, allowing acquisition"
                );
                // Fall through to acquire
            } else if existing.owner == agent_id {
                return LockResult::AlreadyHeld;
            } else {
                return LockResult::Held {
                    owner: existing.owner.clone(),
                };
            }
        }

        locks.insert(
            resource.to_string(),
            LockEntry {
                owner: agent_id.to_string(),
                acquired_at: Instant::now(),
                ttl,
                reason: reason.to_string(),
            },
        );

        debug!(resource = %resource, agent = %agent_id, "Lock acquired");
        LockResult::Acquired
    }

    /// Release a lock. Only the owner can release it.
    pub fn release(&self, resource: &str, agent_id: &str) -> bool {
        let mut locks = self.locks.write();
        if let Some(entry) = locks.get(resource) {
            if entry.owner == agent_id || entry.is_expired() {
                locks.remove(resource);
                debug!(resource = %resource, agent = %agent_id, "Lock released");
                return true;
            }
            warn!(
                resource = %resource,
                agent = %agent_id,
                owner = %entry.owner,
                "Lock release denied: not the owner"
            );
            return false;
        }
        true // Not locked = effectively released
    }

    /// Force-release a lock (admin operation).
    pub fn force_release(&self, resource: &str) -> bool {
        self.locks.write().remove(resource).is_some()
    }

    /// Check if a resource is locked.
    pub fn is_locked(&self, resource: &str) -> bool {
        let locks = self.locks.read();
        locks
            .get(resource)
            .map(|e| !e.is_expired())
            .unwrap_or(false)
    }

    /// Get the owner of a lock.
    pub fn lock_owner(&self, resource: &str) -> Option<AgentId> {
        let locks = self.locks.read();
        locks
            .get(resource)
            .filter(|e| !e.is_expired())
            .map(|e| e.owner.clone())
    }

    /// Release all locks held by a specific agent.
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

    /// Evict all expired locks. Returns count evicted.
    pub fn evict_expired(&self) -> usize {
        let mut locks = self.locks.write();
        let before = locks.len();
        locks.retain(|_, entry| !entry.is_expired());
        before - locks.len()
    }

    /// Get all currently held locks.
    pub fn all_locks(&self) -> Vec<(String, AgentId, String)> {
        self.locks
            .read()
            .iter()
            .filter(|(_, e)| !e.is_expired())
            .map(|(k, e)| (k.clone(), e.owner.clone(), e.reason.clone()))
            .collect()
    }

    /// Number of active locks.
    pub fn lock_count(&self) -> usize {
        self.locks
            .read()
            .values()
            .filter(|e| !e.is_expired())
            .count()
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

// ── Barrier Synchronization ─────────────────────────────────────

/// A synchronization barrier for multi-agent phased execution.
#[derive(Debug, Clone)]
struct BarrierState {
    /// Agents expected to reach the barrier.
    expected: HashSet<AgentId>,
    /// Agents that have arrived.
    arrived: HashSet<AgentId>,
    /// When the barrier was created.
    created_at: Instant,
    /// Timeout for the barrier.
    timeout: Duration,
}

/// Result of arriving at a barrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarrierResult {
    /// Agent registered at the barrier, waiting for others.
    Waiting { arrived: usize, expected: usize },
    /// All agents have arrived, barrier released.
    Released,
    /// Barrier has timed out.
    TimedOut,
    /// Barrier not found.
    NotFound,
}

/// Manages synchronization barriers.
pub struct BarrierManager {
    barriers: RwLock<HashMap<String, BarrierState>>,
}

impl BarrierManager {
    pub fn new() -> Self {
        Self {
            barriers: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new barrier.
    pub fn create_barrier(&self, name: &str, expected_agents: HashSet<AgentId>, timeout: Duration) {
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
    }

    /// An agent arrives at a barrier.
    pub fn arrive(&self, barrier_name: &str, agent_id: &str) -> BarrierResult {
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
            info!(barrier = %barrier_name, "Barrier released — all agents arrived");
            BarrierResult::Released
        } else {
            BarrierResult::Waiting {
                arrived: barrier.arrived.len(),
                expected: barrier.expected.len(),
            }
        }
    }

    /// Check barrier status without arriving.
    pub fn status(&self, barrier_name: &str) -> Option<(usize, usize)> {
        let barriers = self.barriers.read();
        barriers
            .get(barrier_name)
            .map(|b| (b.arrived.len(), b.expected.len()))
    }

    /// Remove a barrier.
    pub fn remove(&self, barrier_name: &str) -> bool {
        self.barriers.write().remove(barrier_name).is_some()
    }

    /// Number of active barriers.
    pub fn count(&self) -> usize {
        self.barriers.read().len()
    }

    /// Clean up timed-out barriers. Returns count removed.
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

// ── Voting / Consensus ──────────────────────────────────────────

/// A vote cast by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub agent_id: AgentId,
    pub value: String,
    pub timestamp: DateTime<Utc>,
}

/// State of a voting session.
#[derive(Debug, Clone)]
struct VotingSession {
    /// The topic / proposal being voted on.
    topic: String,
    /// Who initiated the vote.
    initiator: AgentId,
    /// Eligible voters.
    eligible: HashSet<AgentId>,
    /// Votes cast.
    votes: Vec<Vote>,
    /// When the session was created.
    created_at: Instant,
    /// Voting deadline.
    timeout: Duration,
    /// Required majority fraction (0.0 – 1.0).
    majority: f64,
}

/// Result of a voting session.
#[derive(Debug, Clone, PartialEq)]
pub enum VotingResult {
    /// Vote recorded, session still open.
    Recorded {
        votes_cast: usize,
        votes_needed: usize,
    },
    /// Consensus reached.
    Consensus { winning_value: String, votes: usize },
    /// No consensus (timeout or split vote).
    NoConsensus { tally: HashMap<String, usize> },
    /// Session not found.
    NotFound,
    /// Agent already voted.
    AlreadyVoted,
    /// Session timed out.
    TimedOut,
}

/// Manages voting sessions for consensus.
pub struct VotingManager {
    sessions: RwLock<HashMap<String, VotingSession>>,
}

impl VotingManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Start a voting session.
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
                topic: topic.to_string(),
                initiator: initiator.to_string(),
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
    }

    /// Cast a vote.
    pub fn cast_vote(&self, session_id: &str, agent_id: &str, value: &str) -> VotingResult {
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

        // Check if agent already voted
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

        // Check if consensus is reached
        let eligible_count = session.eligible.len();
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
                return VotingResult::Consensus {
                    winning_value: winning,
                    votes,
                };
            }
        }

        // Check if all eligible agents have voted (no consensus possible for remaining)
        if session.votes.len() >= eligible_count {
            let tally_clone = tally.clone();
            sessions.remove(session_id);
            return VotingResult::NoConsensus { tally: tally_clone };
        }

        VotingResult::Recorded {
            votes_cast: session.votes.len(),
            votes_needed: needed,
        }
    }

    /// Get the current tally for a session.
    pub fn tally(&self, session_id: &str) -> Option<HashMap<String, usize>> {
        let sessions = self.sessions.read();
        sessions
            .get(session_id)
            .map(|s| Self::compute_tally(&s.votes))
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }

    /// Clean up timed-out sessions.
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

// ── Unified Coordinator ─────────────────────────────────────────

/// Unified coordinator providing all coordination primitives.
pub struct Coordinator {
    pub locks: LockManager,
    pub barriers: BarrierManager,
    pub voting: VotingManager,
}

impl Coordinator {
    pub fn new() -> Self {
        Self {
            locks: LockManager::default(),
            barriers: BarrierManager::new(),
            voting: VotingManager::new(),
        }
    }

    pub fn with_lock_ttl(lock_ttl: Duration) -> Self {
        Self {
            locks: LockManager::new(lock_ttl),
            barriers: BarrierManager::new(),
            voting: VotingManager::new(),
        }
    }

    /// Run periodic maintenance (evict expired locks, barriers, sessions).
    pub fn maintenance(&self) -> (usize, usize, usize) {
        let locks = self.locks.evict_expired();
        let barriers = self.barriers.evict_expired();
        let voting = self.voting.evict_expired();
        (locks, barriers, voting)
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe handle to the coordinator.
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

    pub fn barriers(&self) -> &BarrierManager {
        &self.inner.barriers
    }

    pub fn voting(&self) -> &VotingManager {
        &self.inner.voting
    }

    pub fn maintenance(&self) -> (usize, usize, usize) {
        self.inner.maintenance()
    }
}

impl From<Coordinator> for CoordinatorHandle {
    fn from(c: Coordinator) -> Self {
        Self::new(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Lock tests ──────────────────────────────────────────────

    #[test]
    fn lock_acquire_and_release() {
        let mgr = LockManager::default();
        assert_eq!(
            mgr.acquire("file.txt", "agent-1", "editing"),
            LockResult::Acquired
        );
        assert!(mgr.is_locked("file.txt"));
        assert_eq!(mgr.lock_owner("file.txt"), Some("agent-1".to_string()));

        assert!(mgr.release("file.txt", "agent-1"));
        assert!(!mgr.is_locked("file.txt"));
    }

    #[test]
    fn lock_contention() {
        let mgr = LockManager::default();
        assert_eq!(mgr.acquire("res", "a1", "reason"), LockResult::Acquired);
        assert_eq!(
            mgr.acquire("res", "a2", "reason"),
            LockResult::Held {
                owner: "a1".to_string()
            }
        );
    }

    #[test]
    fn lock_reentrant() {
        let mgr = LockManager::default();
        mgr.acquire("res", "a1", "reason");
        assert_eq!(mgr.acquire("res", "a1", "reason"), LockResult::AlreadyHeld);
    }

    #[test]
    fn lock_expiration() {
        let mgr = LockManager::new(Duration::from_millis(1));
        mgr.acquire("res", "a1", "reason");
        std::thread::sleep(Duration::from_millis(10));

        // Expired lock should allow new acquisition
        assert_eq!(mgr.acquire("res", "a2", "new owner"), LockResult::Acquired);
    }

    #[test]
    fn release_wrong_owner_denied() {
        let mgr = LockManager::default();
        mgr.acquire("res", "a1", "reason");
        assert!(!mgr.release("res", "a2"));
    }

    #[test]
    fn release_all_for_agent() {
        let mgr = LockManager::default();
        mgr.acquire("r1", "a1", "r");
        mgr.acquire("r2", "a1", "r");
        mgr.acquire("r3", "a2", "r");

        assert_eq!(mgr.release_all_for_agent("a1"), 2);
        assert_eq!(mgr.lock_count(), 1);
    }

    // ── Barrier tests ───────────────────────────────────────────

    #[test]
    fn barrier_all_arrive() {
        let mgr = BarrierManager::new();
        let agents: HashSet<_> = ["a1", "a2", "a3"].iter().map(|s| s.to_string()).collect();
        mgr.create_barrier("phase1", agents, Duration::from_secs(60));

        assert_eq!(
            mgr.arrive("phase1", "a1"),
            BarrierResult::Waiting {
                arrived: 1,
                expected: 3
            }
        );
        assert_eq!(
            mgr.arrive("phase1", "a2"),
            BarrierResult::Waiting {
                arrived: 2,
                expected: 3
            }
        );
        assert_eq!(mgr.arrive("phase1", "a3"), BarrierResult::Released);
    }

    #[test]
    fn barrier_not_found() {
        let mgr = BarrierManager::new();
        assert_eq!(mgr.arrive("nonexistent", "a1"), BarrierResult::NotFound);
    }

    #[test]
    fn barrier_timeout() {
        let mgr = BarrierManager::new();
        let agents: HashSet<_> = ["a1", "a2"].iter().map(|s| s.to_string()).collect();
        mgr.create_barrier("b", agents, Duration::from_millis(1));

        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(mgr.arrive("b", "a1"), BarrierResult::TimedOut);
    }

    // ── Voting tests ────────────────────────────────────────────

    #[test]
    fn voting_consensus() {
        let mgr = VotingManager::new();
        let eligible: HashSet<_> = ["a1", "a2", "a3"].iter().map(|s| s.to_string()).collect();
        mgr.start_session(
            "v1",
            "Which color?",
            "a1",
            eligible,
            Duration::from_secs(60),
            0.5,
        );

        let r1 = mgr.cast_vote("v1", "a1", "blue");
        assert!(matches!(r1, VotingResult::Recorded { .. }));

        let r2 = mgr.cast_vote("v1", "a2", "blue");
        assert!(matches!(
            r2,
            VotingResult::Consensus {
                winning_value,
                votes: 2
            } if winning_value == "blue"
        ));
    }

    #[test]
    fn voting_no_consensus() {
        let mgr = VotingManager::new();
        let eligible: HashSet<_> = ["a1", "a2", "a3"].iter().map(|s| s.to_string()).collect();
        mgr.start_session("v1", "topic", "a1", eligible, Duration::from_secs(60), 0.8);

        mgr.cast_vote("v1", "a1", "red");
        mgr.cast_vote("v1", "a2", "blue");
        let r3 = mgr.cast_vote("v1", "a3", "green");
        assert!(matches!(r3, VotingResult::NoConsensus { .. }));
    }

    #[test]
    fn voting_already_voted() {
        let mgr = VotingManager::new();
        let eligible: HashSet<_> = ["a1", "a2", "a3"].iter().map(|s| s.to_string()).collect();
        mgr.start_session("v1", "topic", "a1", eligible, Duration::from_secs(60), 1.0);

        mgr.cast_vote("v1", "a1", "yes");
        assert_eq!(mgr.cast_vote("v1", "a1", "no"), VotingResult::AlreadyVoted);
    }

    // ── Coordinator tests ───────────────────────────────────────

    #[test]
    fn coordinator_unified() {
        let coord = Coordinator::new();

        // Locks
        assert_eq!(
            coord.locks.acquire("res", "a1", "test"),
            LockResult::Acquired
        );

        // Barriers
        let agents: HashSet<_> = ["a1"].iter().map(|s| s.to_string()).collect();
        coord
            .barriers
            .create_barrier("b1", agents, Duration::from_secs(60));
        assert_eq!(coord.barriers.arrive("b1", "a1"), BarrierResult::Released);

        // Voting
        let eligible: HashSet<_> = ["a1"].iter().map(|s| s.to_string()).collect();
        coord
            .voting
            .start_session("v1", "topic", "a1", eligible, Duration::from_secs(60), 0.5);
        let r = coord.voting.cast_vote("v1", "a1", "yes");
        assert!(matches!(r, VotingResult::Consensus { .. }));
    }

    #[test]
    fn coordinator_handle() {
        let handle = CoordinatorHandle::new(Coordinator::new());
        assert_eq!(
            handle.locks().acquire("r", "a1", "test"),
            LockResult::Acquired
        );
        assert_eq!(handle.locks().lock_count(), 1);
    }
}
