// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! evaluation harness.
//!
//! The harness lets CI run offline accuracy benchmarks against the
//! current build (HumanEval / MBPP / SWE-bench-lite) and push the
//! results to Prometheus.  ships the trait + a small
//! scripted runner; the real fixture loaders live behind the
//! `evals-fixtures` feature so minimal builds stay lean.
//!
//! The flow is:
//!   1. A runner implementing [`EvalSuite`] enumerates problems.
//!   2. The caller executes each problem with the current agent and
//!      returns the output.
//!   3. The runner's [`EvalSuite::judge`] grades the output and
//!      returns a [`ProblemResult`].
//!   4. [`SuiteReport`] aggregates metrics (accuracy, pass@1,
//!      latency) that can be pushed to Prometheus via the existing
//!      `observability::metrics` pushgateway.

pub mod runner;
pub mod runners;
pub mod suites;
pub mod traits;

pub use runner::{EvalExecutor, SuiteReport, run_suite, run_suite_concurrent};
pub use suites::{humaneval::HumanEvalSuite, mbpp::MbppSuite, swebench::SweBenchLiteSuite};
pub use traits::{EvalProblem, EvalSuite, ProblemResult, Verdict};

#[cfg(feature = "evals-swebench-docker")]
pub use runners::{
    SweBenchDockerConfig, SweBenchDockerExecutor, SweBenchDockerSuite, SweBenchExecutorReport,
    SweBenchInstance, SweBenchTestSummary,
};
