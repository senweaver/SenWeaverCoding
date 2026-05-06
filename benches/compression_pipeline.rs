// Benchmarks the hard-trim stage of the context compression pipeline.
//
// Measures throughput for shrinking 1K / 10K / 100K-token histories to a
// fixed `max_messages` budget.  The LLM-based summarisation stage is not
// measured here (it is bounded by provider latency, not local CPU).

use criterion::{Criterion, criterion_group, criterion_main};
use senweavercoding::agent::context_pipeline::{ContextPipeline, HardTrimStage};
use senweavercoding::providers::traits::ChatMessage;
use std::hint::black_box;

fn make_history(n: usize, avg_chars: usize) -> Vec<ChatMessage> {
    (0..n)
        .map(|i| ChatMessage {
            role: if i % 2 == 0 {
                "user".into()
            } else {
                "assistant".into()
            },
            content: "x".repeat(avg_chars),
            metadata: Default::default(),
        })
        .collect()
}

fn bench_hard_trim_small(c: &mut Criterion) {
    c.bench_function("compression_hard_trim_100_msgs_to_20", |b| {
        b.iter(|| {
            let mut history = make_history(100, 200);
            let pipeline = ContextPipeline::new().push(HardTrimStage { max_messages: 20 });
            let reports = pipeline.run(&mut history, 500, 5_000);
            black_box(reports.len())
        })
    });
}

fn bench_hard_trim_medium(c: &mut Criterion) {
    c.bench_function("compression_hard_trim_1000_msgs_to_50", |b| {
        b.iter(|| {
            let mut history = make_history(1000, 200);
            let pipeline = ContextPipeline::new().push(HardTrimStage { max_messages: 50 });
            let reports = pipeline.run(&mut history, 5_000, 50_000);
            black_box(reports.len())
        })
    });
}

fn bench_hard_trim_large(c: &mut Criterion) {
    c.bench_function("compression_hard_trim_10000_msgs_to_100", |b| {
        b.iter(|| {
            let mut history = make_history(10_000, 200);
            let pipeline = ContextPipeline::new().push(HardTrimStage { max_messages: 100 });
            let reports = pipeline.run(&mut history, 50_000, 500_000);
            black_box(reports.len())
        })
    });
}

fn bench_no_op_path(c: &mut Criterion) {
    // Pipeline runs but history already under target — measures the fast
    // "nothing to do" code path.
    c.bench_function("compression_noop_path", |b| {
        b.iter(|| {
            let mut history = make_history(10, 100);
            let pipeline = ContextPipeline::new().push(HardTrimStage { max_messages: 50 });
            let reports = pipeline.run(&mut history, 10_000, 1_000);
            black_box(reports.len())
        })
    });
}

criterion_group!(
    benches,
    bench_hard_trim_small,
    bench_hard_trim_medium,
    bench_hard_trim_large,
    bench_no_op_path,
);
criterion_main!(benches);
