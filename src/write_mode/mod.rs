// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Write Mode: multi-step autonomous flow planner.
//!
//! Write Mode is the Cursor / Windsurf-style "agentic edit" feature:
//! the user states a high-level goal (e.g. "add a WAL checkpoint task
//! to `memory::sqlite` and cover it with a test") and the agent
//! produces a `WritePlan` composed of `WriteStep`s, executes each
//! step, and verifies the final result.
//!
//! Design goals:
//! * **Real** — no `todo!()` placeholders; every step type has a
//!   concrete executor path (see [`executor`]).
//! * **Bounded** — plans are capped at [`MAX_PLAN_STEPS`] steps and
//!   each step carries a timeout so a runaway planner cannot loop.
//! * **Observable** — every plan / step / verify transition increments
//!   a Prometheus counter (see
//!   [`crate::observability::session_write_mode_metrics`]).
//! * **Three-end parity** — the planner output (`WritePlan`) is pure
//!   data; CLI, TUI, and GUI render it with their own views but the
//!   bytes match.
//!
//! Wiring summary:
//!
//! ```text
//!   user goal ──► WritePlanner::plan ──► WritePlan
//!                                          │
//!                                          ▼
//!                  WriteExecutor::execute(plan) ──► Vec<StepOutcome>
//!                         │
//!                         ├─ ReadFile     → fs::read_to_string
//!                         ├─ GrepSymbol   → SymbolGraph lookup
//!                         ├─ ApplyDiff    → InlineEditRunner ()
//!                         └─ RunTest      → TestRunnerVerifier ()
//! ```

pub mod executor;
pub mod planner;
pub mod prompts;
pub mod types;

pub use executor::{ExecuteError, StepOutcome, WriteExecutor};
pub use planner::{HeuristicPlanner, LlmWritePlanner, WritePlanner};
pub use prompts::build_plan_user_prompt;
pub use types::{PlanContext, VerifyOutcome, WritePlan, WriteStep};

pub const MAX_PLAN_STEPS: usize = 7;
