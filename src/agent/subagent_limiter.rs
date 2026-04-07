// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Subagent Concurrency Limiter - caps parallel sub-agent executions.
//!
//! Prevents resource exhaustion from too many concurrent delegate/swarm tasks
//! by enforcing a configurable maximum.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Configuration for subagent concurrency limits.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentLimitConfig {
    /// Maximum concurrent subagent tasks. Default: 3. Clamped to [1, 8].
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    /// Whether to queue excess tasks or reject them. Default: true (queue).
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

/// Tracks and enforces subagent concurrency limits.
#[derive(Clone)]
pub struct SubagentLimiter {
    active: Arc<AtomicUsize>,
    max_concurrent: usize,
    queue_excess: bool,
}

/// Result of a permit acquisition attempt.
pub enum PermitResult {
    /// Permit granted — proceed with subagent execution.
    Granted(SubagentPermit),
    /// Over limit — task should be queued for later.
    Queued,
    /// Over limit and queuing disabled — task rejected.
    Rejected { active: usize, max: usize },
}

/// RAII guard that releases the permit when dropped.
pub struct SubagentPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for SubagentPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

impl SubagentLimiter {
    pub fn new(config: &SubagentLimitConfig) -> Self {
        let max = config.max_concurrent.clamp(1, 8);
        Self {
            active: Arc::new(AtomicUsize::new(0)),
            max_concurrent: max,
            queue_excess: config.queue_excess,
        }
    }

    /// Try to acquire a permit for a subagent task.
    pub fn try_acquire(&self) -> PermitResult {
        let current = self.active.fetch_add(1, Ordering::SeqCst);
        if current < self.max_concurrent {
            PermitResult::Granted(SubagentPermit {
                active: Arc::clone(&self.active),
            })
        } else {
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.queue_excess {
                PermitResult::Queued
            } else {
                PermitResult::Rejected {
                    active: current,
                    max: self.max_concurrent,
                }
            }
        }
    }

    /// Current number of active subagent tasks.
    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// Maximum allowed concurrent tasks.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Check if at capacity.
    pub fn is_at_capacity(&self) -> bool {
        self.active_count() >= self.max_concurrent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_acquire_release() {
        let limiter = SubagentLimiter::new(&SubagentLimitConfig {
            max_concurrent: 2,
            ..Default::default()
        });

        let p1 = limiter.try_acquire();
        assert!(matches!(p1, PermitResult::Granted(_)));
        assert_eq!(limiter.active_count(), 1);

        let p2 = limiter.try_acquire();
        assert!(matches!(p2, PermitResult::Granted(_)));
        assert_eq!(limiter.active_count(), 2);

        let p3 = limiter.try_acquire();
        assert!(matches!(p3, PermitResult::Queued));
        assert_eq!(limiter.active_count(), 2);

        drop(p1);
        assert_eq!(limiter.active_count(), 1);

        let p4 = limiter.try_acquire();
        assert!(matches!(p4, PermitResult::Granted(_)));

        drop(p2);
        drop(p3);
        drop(p4);
    }

    #[test]
    fn test_reject_mode() {
        let limiter = SubagentLimiter::new(&SubagentLimitConfig {
            max_concurrent: 1,
            queue_excess: false,
        });

        let _p1 = limiter.try_acquire();
        let p2 = limiter.try_acquire();
        assert!(matches!(p2, PermitResult::Rejected { .. }));
    }

    #[test]
    fn test_clamp_range() {
        let limiter = SubagentLimiter::new(&SubagentLimitConfig {
            max_concurrent: 100,
            ..Default::default()
        });
        assert_eq!(limiter.max_concurrent(), 8);

        let limiter = SubagentLimiter::new(&SubagentLimitConfig {
            max_concurrent: 0,
            ..Default::default()
        });
        assert_eq!(limiter.max_concurrent(), 1);
    }

    #[test]
    fn test_is_at_capacity() {
        let limiter = SubagentLimiter::new(&SubagentLimitConfig {
            max_concurrent: 1,
            ..Default::default()
        });

        assert!(!limiter.is_at_capacity());
        let _p = limiter.try_acquire();
        assert!(limiter.is_at_capacity());
    }
}
