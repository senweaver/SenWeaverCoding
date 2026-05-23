// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(feature = "evals-swebench-docker")]
pub mod swebench_docker;

#[cfg(feature = "evals-swebench-docker")]
pub use swebench_docker::{
    SweBenchDockerConfig, SweBenchDockerExecutor, SweBenchDockerSuite, SweBenchExecutorReport,
    SweBenchInstance, SweBenchTestSummary,
};
