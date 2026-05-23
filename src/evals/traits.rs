// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalProblem {
    pub id: String,
    pub prompt: String,

    pub reference: Option<String>,

    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Pass,
    Fail,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemResult {
    pub problem_id: String,
    pub verdict: Verdict,
    pub output: String,
    pub latency_ms: u64,
    pub notes: Option<String>,
}

#[async_trait]
pub trait EvalSuite: Send + Sync {
    fn name(&self) -> &'static str;
    async fn problems(&self) -> Vec<EvalProblem>;
    async fn judge(&self, problem: &EvalProblem, output: &str) -> ProblemResult;
}
