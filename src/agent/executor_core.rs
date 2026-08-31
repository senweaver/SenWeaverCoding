// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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

pub const PACING_GUARD_PREFIXES: [&str; 4] = [
    "[Progress Guard]",
    "[Token Budget]",
    "[Iteration Ceiling]",
    "[Time Budget]",
];

pub fn is_pacing_guard_message(content: &str) -> bool {
    let trimmed = content.trim_start();
    PACING_GUARD_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

#[derive(Debug, Clone)]
pub struct PacingBudget {
    pub no_progress_limit: usize,
    pub absolute_iteration_limit: usize,
    pub total_timeout: Option<Duration>,
    pub token_soft_cap: u64,
    pub token_hard_cap: u64,
}

#[derive(Debug, Clone)]
pub struct PacingGovernor {
    budget: PacingBudget,
    turn_started: Instant,
    iteration: usize,
    no_progress_streak: usize,
    tokens_since_progress: u64,
    total_generated_tokens: u64,
    no_progress_warned: bool,
    token_soft_warned: bool,
    absolute_nudge_20: bool,
    absolute_nudge_50: bool,
    absolute_warned: bool,
    timeout_warned: bool,
}

impl PacingGovernor {
    pub fn new(budget: PacingBudget) -> Self {
        let budget = PacingBudget {
            no_progress_limit: budget.no_progress_limit.max(1),
            absolute_iteration_limit: budget.absolute_iteration_limit.max(1),
            ..budget
        };
        Self {
            budget,
            turn_started: Instant::now(),
            iteration: 0,
            no_progress_streak: 0,
            tokens_since_progress: 0,
            total_generated_tokens: 0,
            no_progress_warned: false,
            token_soft_warned: false,
            absolute_nudge_20: false,
            absolute_nudge_50: false,
            absolute_warned: false,
            timeout_warned: false,
        }
    }

    pub fn tick(&mut self) -> Result<usize, PacingExceeded> {
        self.iteration += 1;
        self.no_progress_streak += 1;
        if self.iteration > self.budget.absolute_iteration_limit {
            return Err(PacingExceeded::AbsoluteIterations {
                limit: self.budget.absolute_iteration_limit,
            });
        }
        if self.no_progress_streak > self.budget.no_progress_limit {
            return Err(PacingExceeded::IterationBudget {
                limit: self.budget.no_progress_limit,
            });
        }
        if self.budget.token_hard_cap > 0
            && self.tokens_since_progress >= self.budget.token_hard_cap
        {
            return Err(PacingExceeded::TokenBudget {
                used: self.tokens_since_progress,
                limit: self.budget.token_hard_cap,
            });
        }
        if let Some(total) = self.budget.total_timeout {
            if self.turn_started.elapsed() > total {
                return Err(PacingExceeded::TotalTimeout { limit: total });
            }
        }
        Ok(self.iteration)
    }

    pub fn note_progress(&mut self) {
        self.no_progress_streak = 0;
        self.tokens_since_progress = 0;
        self.no_progress_warned = false;
        self.token_soft_warned = false;
    }

    pub fn total_timeout_exceeded(&self) -> Option<Duration> {
        let total = self.budget.total_timeout?;
        if self.turn_started.elapsed() > total {
            Some(total)
        } else {
            None
        }
    }

    pub fn note_token_progress(&mut self) {
        self.tokens_since_progress = 0;
        self.token_soft_warned = false;
    }

    pub fn record_generated_tokens(&mut self, tokens: u64) {
        self.tokens_since_progress = self.tokens_since_progress.saturating_add(tokens);
        self.total_generated_tokens = self.total_generated_tokens.saturating_add(tokens);
    }

    pub fn drain_warnings(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        let nudge_at = (self.budget.no_progress_limit / 2).max(1);
        if !self.no_progress_warned && self.no_progress_streak >= nudge_at {
            self.no_progress_warned = true;
            warnings.push(format!(
                "[Progress Guard] {} consecutive iterations have passed without new information \
                 (successful but repeated exploration still accumulates). The turn will stop safely \
                 after {} consecutive no-progress iterations. Re-reading covered ranges or repeating \
                 the same search does not reset this counter. Step back and change strategy: page \
                 into uncovered line ranges, try a different pattern or tool, make a mutation, or \
                 ask the user for guidance.",
                self.no_progress_streak, self.budget.no_progress_limit
            ));
        }
        if !self.token_soft_warned
            && self.budget.token_soft_cap > 0
            && self.tokens_since_progress >= self.budget.token_soft_cap
        {
            self.token_soft_warned = true;
            warnings.push(format!(
                "[Token Budget] Roughly {} tokens have been generated since the last \
                 new information; the turn will stop safely at {}. Repeated successful \
                 exploration that adds no coverage still accumulates. Stop broad re-reads \
                 of the same ranges, pick the single most promising next action, or ask \
                 the user for guidance.",
                self.tokens_since_progress, self.budget.token_hard_cap
            ));
        }
        let absolute_nudge_20_at = (self.budget.absolute_iteration_limit / 5).max(1);
        let absolute_nudge_50_at = (self.budget.absolute_iteration_limit / 2).max(1);
        let absolute_nudge_at = self
            .budget
            .absolute_iteration_limit
            .saturating_sub(self.budget.absolute_iteration_limit / 10)
            .max(1);
        if !self.absolute_nudge_20
            && self.iteration >= absolute_nudge_20_at
            && absolute_nudge_20_at < absolute_nudge_50_at
        {
            self.absolute_nudge_20 = true;
            warnings.push(format!(
                "[Iteration Ceiling] This turn has used {} of the {} allowed iterations (20%). \
                 Start wrapping up: prefer mutations and uncovered reads over re-exploring \
                 the same files or queries, then summarize remaining work so it can continue \
                 in the next turn if needed.",
                self.iteration, self.budget.absolute_iteration_limit
            ));
        }
        if !self.absolute_nudge_50
            && self.iteration >= absolute_nudge_50_at
            && absolute_nudge_50_at < absolute_nudge_at
        {
            self.absolute_nudge_50 = true;
            warnings.push(format!(
                "[Iteration Ceiling] This turn has used {} of the {} allowed iterations (50%). \
                 Prioritize finishing the user's task: complete the most important remaining \
                 step, avoid repeating covered exploration, and prepare a concise handoff \
                 for anything that will not fit in this turn.",
                self.iteration, self.budget.absolute_iteration_limit
            ));
        }
        if !self.absolute_warned && self.iteration >= absolute_nudge_at {
            self.absolute_warned = true;
            warnings.push(format!(
                "[Iteration Ceiling] This turn has used {} of the {} allowed iterations. \
                 Prioritize finishing the user's task now: complete the most important \
                 remaining step, then summarize what was done and what still needs doing \
                 so work can continue seamlessly in the next turn.",
                self.iteration, self.budget.absolute_iteration_limit
            ));
        }
        if let Some(total) = self.budget.total_timeout {
            let elapsed = self.turn_started.elapsed();
            if !self.timeout_warned && elapsed >= total.mul_f32(0.8) {
                self.timeout_warned = true;
                warnings.push(format!(
                    "[Time Budget] This turn has been running for {}s of its {}s limit. \
                     Prioritize finishing the user's task now: complete the most important \
                     remaining step, then summarize progress so work can continue in the \
                     next turn.",
                    elapsed.as_secs(),
                    total.as_secs()
                ));
            }
        }
        warnings
    }

    pub fn iteration(&self) -> usize {
        self.iteration
    }

    pub fn total_generated_tokens(&self) -> u64 {
        self.total_generated_tokens
    }

    pub fn remaining_iterations(&self) -> usize {
        let no_progress = self
            .budget
            .no_progress_limit
            .saturating_sub(self.no_progress_streak);
        let absolute = self
            .budget
            .absolute_iteration_limit
            .saturating_sub(self.iteration);
        no_progress.min(absolute)
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum PacingExceeded {
    #[error("no forward progress for {limit} consecutive iterations")]
    IterationBudget { limit: usize },
    #[error("reached the absolute per-turn iteration ceiling of {limit}")]
    AbsoluteIterations { limit: usize },
    #[error("exceeded total turn timeout (limit={limit:?})")]
    TotalTimeout { limit: Duration },
    #[error("generated ~{used} tokens without forward progress (hard cap {limit})")]
    TokenBudget { used: u64, limit: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchPhase {
    pub start: usize,
    pub end: usize,
    pub parallel: bool,
}

pub fn plan_dispatch_phases(
    tool_names: &[impl AsRef<str>],
    needs_approval: impl Fn(&str) -> bool,
    max_concurrency: usize,
) -> Vec<DispatchPhase> {
    let len = tool_names.len();
    if len == 0 {
        return Vec::new();
    }
    if len == 1 || max_concurrency <= 1 {
        return vec![DispatchPhase {
            start: 0,
            end: len,
            parallel: false,
        }];
    }
    let shell_count = tool_names
        .iter()
        .filter(|n| crate::agent::tool_handler::outcome::is_command_execution_tool(n.as_ref()))
        .count();
    let shell_is_barrier = shell_count >= 2;
    let is_barrier = |name: &str| {
        name == "tool_search"
            || crate::agent::mode::effects::is_file_mutation_tool(name)
            || needs_approval(name)
            || (shell_is_barrier
                && crate::agent::tool_handler::outcome::is_command_execution_tool(name))
    };
    let mut phases: Vec<DispatchPhase> = Vec::new();
    let mut run_start: Option<usize> = None;
    for (idx, name) in tool_names.iter().enumerate() {
        if is_barrier(name.as_ref()) {
            if let Some(start) = run_start.take() {
                phases.push(DispatchPhase {
                    start,
                    end: idx,
                    parallel: idx - start > 1,
                });
            }
            phases.push(DispatchPhase {
                start: idx,
                end: idx + 1,
                parallel: false,
            });
        } else if run_start.is_none() {
            run_start = Some(idx);
        }
    }
    if let Some(start) = run_start.take() {
        phases.push(DispatchPhase {
            start,
            end: len,
            parallel: len - start > 1,
        });
    }
    if phases.iter().all(|p| !p.parallel) {
        return vec![DispatchPhase {
            start: 0,
            end: len,
            parallel: false,
        }];
    }
    phases
}
