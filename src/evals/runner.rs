// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Generic suite runner and Prometheus-friendly report struct.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::traits::{EvalProblem, EvalSuite, ProblemResult, Verdict};

#[async_trait]
pub trait EvalExecutor: Send + Sync {
    async fn run(&self, problem: &EvalProblem) -> Result<String, anyhow::Error>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteReport {
    pub suite: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errored: usize,
    pub pass_at_1: f64,
    pub avg_latency_ms: f64,
    pub results: Vec<ProblemResult>,
}

impl SuiteReport {
    pub fn empty(suite: &str) -> Self {
        Self {
            suite: suite.to_string(),
            total: 0,
            passed: 0,
            failed: 0,
            errored: 0,
            pass_at_1: 0.0,
            avg_latency_ms: 0.0,
            results: Vec::new(),
        }
    }

    pub fn push(&mut self, r: ProblemResult) {
        match r.verdict {
            Verdict::Pass => {
                self.passed += 1;
                crate::observability::subsystem_metrics::incr_evals_pass();
            }
            Verdict::Fail => {
                self.failed += 1;
                crate::observability::subsystem_metrics::incr_evals_fail();
            }
            Verdict::Error => self.errored += 1,
        }
        self.total += 1;

        let n = self.total as f64;
        self.avg_latency_ms = ((self.avg_latency_ms * (n - 1.0)) + r.latency_ms as f64) / n;
        self.pass_at_1 = self.passed as f64 / n;
        crate::observability::subsystem_metrics::set_evals_pass_at_1(self.pass_at_1);
        self.results.push(r);
    }
}

pub async fn run_suite(suite: &dyn EvalSuite, executor: &dyn EvalExecutor) -> SuiteReport {
    let suite_started = Instant::now();
    let problems = suite.problems().await;
    let mut report = SuiteReport::empty(suite.name());
    for p in problems {
        let start = Instant::now();
        match executor.run(&p).await {
            Ok(output) => {
                let mut result = suite.judge(&p, &output).await;
                result.latency_ms = start.elapsed().as_millis() as u64;
                report.push(result);
            }
            Err(e) => {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                report.push(ProblemResult {
                    problem_id: p.id.clone(),
                    verdict: Verdict::Error,
                    output: String::new(),
                    latency_ms: elapsed_ms,
                    notes: Some(format!("executor error: {e}")),
                });
            }
        }
    }
    crate::observability::session_write_mode_metrics::observe_evals_suite_seconds(
        suite.name(),
        suite_started.elapsed().as_secs_f64(),
    );
    report
}

pub async fn run_suite_concurrent(
    suite: &(dyn EvalSuite + 'static),
    executor: std::sync::Arc<dyn EvalExecutor>,
    concurrency: usize,
) -> SuiteReport {
    use futures_util::stream::{FuturesUnordered, StreamExt};

    let suite_started = Instant::now();
    let problems = suite.problems().await;
    let mut report = SuiteReport::empty(suite.name());
    let concurrency = concurrency.max(1);

    let mut iter = problems.into_iter();
    let mut inflight: FuturesUnordered<_> = (&mut iter)
        .take(concurrency)
        .map(|p| run_one(suite, executor.clone(), p))
        .collect();

    let mut results: Vec<ProblemResult> = Vec::new();
    while let Some(res) = inflight.next().await {
        results.push(res);
        if let Some(p) = iter.next() {
            inflight.push(run_one(suite, executor.clone(), p));
        }
    }

    results.sort_by(|a, b| a.problem_id.cmp(&b.problem_id));
    for r in results {
        report.push(r);
    }
    crate::observability::session_write_mode_metrics::observe_evals_suite_seconds(
        suite.name(),
        suite_started.elapsed().as_secs_f64(),
    );
    report
}

fn run_one<'a>(
    suite: &'a (dyn EvalSuite + 'static),
    executor: std::sync::Arc<dyn EvalExecutor>,
    problem: EvalProblem,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProblemResult> + Send + 'a>> {
    Box::pin(async move {
        let start = Instant::now();
        match executor.run(&problem).await {
            Ok(output) => {
                let mut r = suite.judge(&problem, &output).await;
                r.latency_ms = start.elapsed().as_millis() as u64;
                r
            }
            Err(e) => ProblemResult {
                problem_id: problem.id.clone(),
                verdict: Verdict::Error,
                output: String::new(),
                latency_ms: start.elapsed().as_millis() as u64,
                notes: Some(format!("executor error: {e}")),
            },
        }
    })
}
