// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Parallel tool-dispatch bench (Phase 4 D1.2).
//!
//! Wires into the real
//! [`senweavercoding::agent::turn_engine::tool_exec::ParallelToolExec`]
//! trait via the [`JoinAllExec`] reference implementation.  The
//! previous version spun a hard-coded `join_all(sleep)` loop which
//! told us nothing about the agent-loop hot path; this version
//! dispatches through the same trait the production loop uses, so
//! regressions in the dispatch layer surface in the bench numbers.
//!
//! Run: `cargo bench --bench parallel_tools`

use async_trait::async_trait;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

use senweavercoding::agent::turn_engine::tool_exec::{
    JoinAllExec, ParallelToolCall, ParallelToolExec,
};
use senweavercoding::tools::{Tool, ToolResult};

struct FakeDelayTool {
    name: String,
}

#[async_trait]
impl Tool for FakeDelayTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "fake bench tool"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(ToolResult {
            output: "ok".into(),
            success: true,
            error: None,
        })
    }
}

fn build_calls(n: usize) -> Vec<ParallelToolCall> {
    (0..n)
        .map(|_| ParallelToolCall {
            name: "fake_delay".into(),
            args: serde_json::json!({}),
            simulated_latency: None,
        })
        .collect()
}

fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(FakeDelayTool {
        name: "fake_delay".into(),
    })]
}

fn bench_parallel_dispatch(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let exec = JoinAllExec;
    let tools = tools();

    let mut group = c.benchmark_group("parallel_tools_join_all_exec");
    for n in [1usize, 2, 4, 8] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("calls", n), &n, |b, &n| {
            b.iter(|| {
                rt.block_on(async {
                    let outcomes = exec.run(black_box(&tools), build_calls(n)).await;
                    black_box(outcomes);
                });
            });
        });
    }
    group.finish();
}

fn bench_sequential_baseline(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tools = tools();

    c.bench_function("parallel_tools_sequential_8x10ms_real_tool", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut oks = 0usize;
                for _ in 0..8 {
                    let res = tools[0].execute(serde_json::json!({})).await.unwrap();
                    if res.success {
                        oks += 1;
                    }
                }
                black_box(oks)
            })
        })
    });
}

criterion_group!(benches, bench_parallel_dispatch, bench_sequential_baseline);
criterion_main!(benches);
