// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;

use crate::evals::traits::{EvalProblem, EvalSuite, ProblemResult, Verdict};

#[derive(Debug, Clone)]
pub struct HumanEvalSuite {
    problems: Vec<EvalProblem>,
}

impl HumanEvalSuite {

    pub fn with_demo_problems() -> Self {
        Self {
            problems: vec![
                EvalProblem {
                    id: "HE/add".into(),
                    prompt: "Return the sum of two integers.".into(),
                    reference: Some("def add(a, b): return a + b".into()),
                    metadata: Default::default(),
                },
                EvalProblem {
                    id: "HE/max".into(),
                    prompt: "Return the max of a list of integers.".into(),
                    reference: Some("def max_of(xs): return max(xs)".into()),
                    metadata: Default::default(),
                },
            ],
        }
    }

    pub fn from_problems(problems: Vec<EvalProblem>) -> Self {
        Self { problems }
    }

    pub fn from_jsonl(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())?;
        let mut problems = Vec::new();
        for (lineno, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("humaneval line {}: {e}", lineno + 1),
                )
            })?;
            let id = v
                .get("task_id")
                .or_else(|| v.get("id"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let prompt = v
                .get("prompt")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let reference = v
                .get("canonical_solution")
                .or_else(|| v.get("reference"))
                .and_then(|x| x.as_str())
                .map(str::to_owned);
            if id.is_empty() || prompt.is_empty() {
                continue;
            }
            problems.push(EvalProblem {
                id,
                prompt,
                reference,
                metadata: Default::default(),
            });
        }
        Ok(Self { problems })
    }
}

#[async_trait]
impl EvalSuite for HumanEvalSuite {
    fn name(&self) -> &'static str {
        "humaneval"
    }
    async fn problems(&self) -> Vec<EvalProblem> {
        self.problems.clone()
    }
    async fn judge(&self, problem: &EvalProblem, output: &str) -> ProblemResult {
        let reference = problem.reference.as_deref().unwrap_or("");
        let verdict = if !reference.is_empty() && output.trim() == reference.trim() {
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
