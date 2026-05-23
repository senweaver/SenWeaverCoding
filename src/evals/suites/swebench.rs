// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;

use crate::evals::traits::{EvalProblem, EvalSuite, ProblemResult, Verdict};

#[derive(Debug, Clone)]
pub struct SweBenchLiteSuite {
    problems: Vec<EvalProblem>,
}

impl SweBenchLiteSuite {
    pub fn with_demo_problems() -> Self {
        Self {
            problems: vec![EvalProblem {
                id: "SWE/demo-1".into(),
                prompt: "Fix the failing test in the repo.".into(),
                reference: Some("patch-applied".into()),
                metadata: Default::default(),
            }],
        }
    }

    pub fn from_problems(problems: Vec<EvalProblem>) -> Self {
        Self { problems }
    }
}

#[async_trait]
impl EvalSuite for SweBenchLiteSuite {
    fn name(&self) -> &'static str {
        "swebench-lite"
    }
    async fn problems(&self) -> Vec<EvalProblem> {
        self.problems.clone()
    }
    async fn judge(&self, problem: &EvalProblem, output: &str) -> ProblemResult {

        let reference = problem.reference.as_deref().unwrap_or("");
        let verdict = if !reference.is_empty() && output.contains(reference) {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
        ProblemResult {
            problem_id: problem.id.clone(),
            verdict,
            output: output.to_string(),
            latency_ms: 0,
            notes: None,
        }
    }
}
