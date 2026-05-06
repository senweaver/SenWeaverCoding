// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `ExecutorCore` — shared orchestration primitives used by the two tool
//! loops that existed historically in this codebase (`run_tool_call_loop`
//! in [`crate::agent::loop_`] and the inline loop in
//! [`crate::agent::agent::Agent::turn_streamed`]).
//!
//! The goal of this module is to **eliminate the semantic drift risk**
//! that comes from maintaining two parallel implementations of:
//!
//! 1. **Parallel tool scheduling** — bounded concurrency semaphore +
//!    `join_all` with cancellation propagation.
//! 2. **Loop detection / dedup** — detect when the LLM gets stuck on the
//!    same tool call signature and force an intervention.
//! 3. **Turn metrics RAII** — `sen_turns_total` / `sen_last_turn_duration_secs`
//!    counters that must fire on every exit path (success, error, panic).
//! 4. **Pacing governor** — budget tracking for `max_iterations` and
//!    `step_timeout_secs`.
//!
//! Only [`run_tool_call_loop`] uses this module today; [`Agent::turn_streamed`]
//! will be collapsed onto `AgentLoopCore` in PR **M2**, which in turn
//! consumes these primitives.  Keeping the wrappers here means a bug fix
//! in one path automatically benefits the other.
//!
//! [`run_tool_call_loop`]: crate::agent::loop_::run_tool_call_loop

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
    max_iterations: usize,
    step_timeout: Option<Duration>,
    total_timeout: Option<Duration>,
    turn_started: Instant,
    iteration: usize,
}

impl PacingGovernor {
    pub fn new(
        max_iterations: usize,
        step_timeout: Option<Duration>,
        total_timeout: Option<Duration>,
    ) -> Self {
        Self {
            max_iterations: max_iterations.max(1),
            step_timeout,
            total_timeout,
            turn_started: Instant::now(),
            iteration: 0,
        }
    }

    pub fn tick(&mut self) -> Result<usize, PacingExceeded> {
        self.iteration += 1;
        if self.iteration > self.max_iterations {
            return Err(PacingExceeded::IterationBudget {
                limit: self.max_iterations,
            });
        }
        if let Some(total) = self.total_timeout {
            if self.turn_started.elapsed() > total {
                return Err(PacingExceeded::TotalTimeout { limit: total });
            }
        }
        Ok(self.iteration)
    }

    pub fn step_deadline(&self) -> Option<Instant> {
        self.step_timeout.map(|d| Instant::now() + d)
    }

    pub fn iteration(&self) -> usize {
        self.iteration
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum PacingExceeded {
    #[error("exceeded iteration budget (limit={limit})")]
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
