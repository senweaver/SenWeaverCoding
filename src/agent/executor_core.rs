// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::observability::agent_metrics::{self, inc_turns};

#[must_use = "dropping the guard emits the metric; bind with `let _g = ...`"]
pub struct TurnMetricsGuard {
    started: Instant,
    status: &'static str,
    finalized: bool,
}

impl TurnMetricsGuard {

    pub fn start() -> Self {
        Self {
            started: Instant::now(),
            status: "error",
            finalized: false,
        }
    }

    pub fn mark_ok(&mut self) {
        self.status = "ok";
    }

    pub fn mark_status(&mut self, status: &'static str) {
        self.status = status;
    }

    pub fn finish(mut self) {
        self.emit();
        self.finalized = true;
    }

    fn emit(&self) {
        if let Some(services) = crate::services::try_get_services() {
            inc_turns(&services.agent_metrics, self.status);
            let duration = self.started.elapsed().as_secs_f64();
            services.agent_metrics.set_gauge(
                "sen_last_turn_duration_secs",
                agent_metrics::LabelSet::new(vec![("status", self.status)]),
                duration,
            );
        }
    }
}

impl Drop for TurnMetricsGuard {
    fn drop(&mut self) {
        if !self.finalized {
            self.emit();
        }
    }
}

#[derive(Debug, Default)]
pub struct ToolLoopDedup {

    seen_signatures: HashMap<String, u32>,

    consecutive_all_repeat: u32,
}

impl ToolLoopDedup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_batch<I, F>(&mut self, calls: I, signature_fn: F) -> bool
    where
        I: IntoIterator,
        F: Fn(&I::Item) -> String,
    {
        let mut saw_new = false;
        let mut count = 0usize;
        for call in calls {
            count += 1;
            let sig = signature_fn(&call);
            let entry = self.seen_signatures.entry(sig).or_insert(0);
            if *entry == 0 {
                saw_new = true;
            }
            *entry += 1;
        }
        if count == 0 {
            return false;
        }
        if saw_new {
            self.consecutive_all_repeat = 0;
            false
        } else {
            self.consecutive_all_repeat += 1;
            true
        }
    }

    pub fn consecutive_all_repeat(&self) -> u32 {
        self.consecutive_all_repeat
    }

    pub fn distinct_signatures(&self) -> usize {
        self.seen_signatures.len()
    }

    pub fn reset(&mut self) {
        self.seen_signatures.clear();
        self.consecutive_all_repeat = 0;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PacingGovernor {
    no_progress_limit: usize,
    step_timeout: Option<Duration>,
    total_timeout: Option<Duration>,
    turn_started: Instant,
    iteration: usize,
    no_progress_streak: usize,
}

impl PacingGovernor {
    pub fn new(
        no_progress_limit: usize,
        step_timeout: Option<Duration>,
        total_timeout: Option<Duration>,
    ) -> Self {
        Self {
            no_progress_limit: no_progress_limit.max(1),
            step_timeout,
            total_timeout,
            turn_started: Instant::now(),
            iteration: 0,
            no_progress_streak: 0,
        }
    }

    pub fn tick(&mut self) -> Result<usize, PacingExceeded> {
        self.iteration += 1;
        self.no_progress_streak += 1;
        if self.no_progress_streak > self.no_progress_limit {
            return Err(PacingExceeded::IterationBudget {
                limit: self.no_progress_limit,
            });
        }
        if let Some(total) = self.total_timeout {
            if self.turn_started.elapsed() > total {
                return Err(PacingExceeded::TotalTimeout { limit: total });
            }
        }
        Ok(self.iteration)
    }

    pub fn note_progress(&mut self) {
        self.no_progress_streak = 0;
    }

    pub fn step_deadline(&self) -> Option<Instant> {
        self.step_timeout.map(|d| Instant::now() + d)
    }

    pub fn iteration(&self) -> usize {
        self.iteration
    }

    pub fn remaining_iterations(&self) -> usize {
        self.no_progress_limit.saturating_sub(self.no_progress_streak)
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum PacingExceeded {
    #[error("no forward progress for {limit} consecutive iterations")]
    IterationBudget { limit: usize },
    #[error("exceeded total turn timeout (limit={limit:?})")]
    TotalTimeout { limit: Duration },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {

    Sequential,

    Parallel { max_concurrency: usize },
}

impl DispatchMode {

    pub fn select(
        tool_names: &[impl AsRef<str>],
        needs_approval: impl Fn(&str) -> bool,
        max_concurrency: usize,
    ) -> Self {
        if tool_names.len() <= 1 {
            return DispatchMode::Sequential;
        }
        if tool_names.iter().any(|n| n.as_ref() == "tool_search") {
            return DispatchMode::Sequential;
        }
        if tool_names.iter().any(|n| needs_approval(n.as_ref())) {
            return DispatchMode::Sequential;
        }
        DispatchMode::Parallel {
            max_concurrency: max_concurrency.max(1),
        }
    }
}
