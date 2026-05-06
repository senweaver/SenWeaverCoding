// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Provider-plumbing micro-benchmarks (Phase 3 D8).
//!
//! Three groups, all CPU-bound (no network IO), exercising the
//! shared `providers::core` utilities that run on the hot path of
//! every outbound LLM call:
//!
//! - `fingerprint` — idempotency-key computation (SHA-256 over the
//!   canonicalised payload).
//! - `sse_parse` — SseParser push/next cycle against a synthetic
//!   32 KiB OpenAI-style delta stream.
//! - `rate_limit` — TokenBucket acquire loop.
//!
//! Run: `cargo bench --bench provider_request`.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use senweavercoding::providers::core::idempotency::fingerprint_json;
use senweavercoding::providers::core::rate_limit::TokenBucket;
use senweavercoding::providers::core::sse::SseParser;

fn synthetic_messages(n: usize) -> serde_json::Value {
    let mut arr = Vec::with_capacity(n);
    for i in 0..n {
        arr.push(serde_json::json!({
            "role": if i % 2 == 0 { "user" } else { "assistant" },
            "content": format!("message-{i} with some content that varies a bit"),
        }));
    }
    serde_json::Value::Array(arr)
}

fn synthetic_sse_chunk(events: usize) -> Vec<u8> {
    let mut out = String::new();
    for i in 0..events {
        out.push_str(&format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"token-{i}\"}}}}]}}\n\n"
        ));
    }
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

fn bench_fingerprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("provider_fingerprint");
    for &n in &[4usize, 32, 256] {
        let msgs = synthetic_messages(n);
        let tools = serde_json::json!([]);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let k = fingerprint_json(
                    black_box("openai"),
                    black_box("gpt-4o-mini"),
                    black_box(&msgs),
                    black_box(&tools),
                );
                black_box(k);
            });
        });
    }
    group.finish();
}

fn bench_sse_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("provider_sse_parse");
    for &n in &[16usize, 128, 1024] {
        let chunk = synthetic_sse_chunk(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut p = SseParser::new();
                p.push(black_box(&chunk));
                let mut count = 0usize;
                while let Some(ev) = p.next() {
                    black_box(&ev);
                    count += 1;
                }
                black_box(count);
            });
        });
    }
    group.finish();
}

fn bench_rate_limit(c: &mut Criterion) {
    c.bench_function("provider_token_bucket_1k_acquires", |b| {
        b.iter(|| {
            let mut tb = TokenBucket::new(1000.0, 10_000.0);
            let mut granted = 0usize;
            for _ in 0..1000 {
                if tb.try_acquire(black_box(1.0)) {
                    granted += 1;
                }
            }
            black_box(granted);
        });
    });
}

criterion_group!(
    benches,
    bench_fingerprint,
    bench_sse_parse,
    bench_rate_limit
);
criterion_main!(benches);
