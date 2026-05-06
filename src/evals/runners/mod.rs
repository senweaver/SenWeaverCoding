// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! heavyweight, integration-style eval runners.
//!
//! The flat `evals::suites::*` modules ship inline / smoke fixtures
//! suitable for `cargo test`.  Real benchmarks (SWE-bench, LiveCodeBench,
//! BigCodeBench, …) need external dependencies — Docker images, repo
//! checkouts, GPU runners — and produce **real scores** that we want to
//! land in the repo via the nightly CI workflow.  That heavier surface
//! lives here so the default build stays small.
//!
//! Each runner is gated behind its own feature so a minimal `cargo
//! check` does not pay any extra cost:
//!
//! - [`swebench_docker`] (`evals-swebench-docker`): drives the upstream
//!   SWE-bench-Lite Docker harness via `docker run`, applies the
//!   agent's predicted patch, and grades the result against the
//!   instance's `FAIL_TO_PASS` / `PASS_TO_PASS` test sets.

#[cfg(feature = "evals-swebench-docker")]
pub mod swebench_docker;

#[cfg(feature = "evals-swebench-docker")]
pub use swebench_docker::{
    SweBenchDockerConfig, SweBenchDockerExecutor, SweBenchDockerSuite, SweBenchExecutorReport,
    SweBenchInstance, SweBenchTestSummary,
};
