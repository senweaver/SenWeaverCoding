// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::config::Config;

use super::runner::{EvalExecutor, SuiteReport, run_suite_concurrent};
use super::suites::{humaneval::HumanEvalSuite, mbpp::MbppSuite, swebench::SweBenchLiteSuite};
use super::traits::{EvalProblem, EvalSuite};

pub struct AgentEvalExecutor {
    pub config: Config,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: f64,
    pub timeout_secs: u64,
}

#[async_trait]
impl EvalExecutor for AgentEvalExecutor {
    async fn run(&self, problem: &EvalProblem) -> Result<String, anyhow::Error> {
        let fut = crate::agent::run(
            self.config.clone(),
            Some(problem.prompt.clone()),
            self.provider.clone(),
            self.model.clone(),
            self.temperature,
            Vec::new(),
            false,
            None,
            None,
            None,
        );
        match tokio::time::timeout(Duration::from_secs(self.timeout_secs.max(1)), Box::pin(fut)).await
        {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!(
                "eval problem '{}' timed out after {}s",
                problem.id,
                self.timeout_secs
            )),
        }
    }
}

pub fn build_suite(name: &str) -> anyhow::Result<Box<dyn EvalSuite>> {
    match name.trim().to_lowercase().as_str() {
        "humaneval" | "human-eval" => Ok(Box::new(HumanEvalSuite::with_demo_problems())),
        "mbpp" => Ok(Box::new(MbppSuite::with_demo_problems())),
        "swebench" | "swebench-lite" | "swe-bench" => {
            Ok(Box::new(SweBenchLiteSuite::with_demo_problems()))
        }
        other => anyhow::bail!(
            "unknown eval suite '{other}' (expected one of: humaneval, mbpp, swebench)"
        ),
    }
}

pub async fn run_agent_suite(
    suite_name: &str,
    executor: AgentEvalExecutor,
    concurrency: usize,
) -> anyhow::Result<SuiteReport> {
    let suite = build_suite(suite_name)?;
    let executor: Arc<dyn EvalExecutor> = Arc::new(executor);
    Ok(run_suite_concurrent(suite.as_ref(), executor, concurrency.max(1)).await)
}
