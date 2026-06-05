// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod agent_executor;
pub mod runner;
pub mod runners;
pub mod suites;
pub mod traits;

pub use agent_executor::{AgentEvalExecutor, build_suite, run_agent_suite};
pub use runner::{EvalExecutor, SuiteReport, run_suite, run_suite_concurrent};
pub use suites::{humaneval::HumanEvalSuite, mbpp::MbppSuite, swebench::SweBenchLiteSuite};
pub use traits::{EvalProblem, EvalSuite, ProblemResult, Verdict};

#[cfg(feature = "evals-swebench-docker")]
pub use runners::{
    SweBenchDockerConfig, SweBenchDockerExecutor, SweBenchDockerSuite, SweBenchExecutorReport,
    SweBenchInstance, SweBenchTestSummary,
};
