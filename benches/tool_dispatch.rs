// Benchmark: tool-spec serialization caching.
//
// Validates that the `ToolSpecCache` eliminates redundant serialization work
// compared to the naive "serialize every iteration" approach.
//
// Run: `cargo bench --bench tool_dispatch`

use criterion::{Criterion, criterion_group, criterion_main};
use senweavercoding::tools::spec_cache::ToolSpecCache;
use senweavercoding::tools::traits::ToolSpec;
use std::hint::black_box;

fn make_specs(n: usize) -> Vec<ToolSpec> {
    (0..n)
        .map(|i| ToolSpec {
            name: format!("tool_{i}"),
            description: format!("Deterministic test tool number {i}"),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "arg1": {"type": "string"},
                    "arg2": {"type": "integer"},
                    "arg3": {"type": "array", "items": {"type": "string"}}
                }
            }),
        })
        .collect()
}

fn bench_naive_serialize_60_tools(c: &mut Criterion) {
    let specs = make_specs(60);
    c.bench_function("serialize_60_tools_every_call", |b| {
        b.iter(|| {
            let s = serde_json::to_string(&specs).unwrap();
            black_box(s.len())
        })
    });
}

fn bench_cached_serialize_60_tools(c: &mut Criterion) {
    let specs = make_specs(60);
    let cache = ToolSpecCache::new();
    c.bench_function("serialize_60_tools_cached", |b| {
        b.iter(|| {
            let arc = cache.get_or_compute("openai", &specs, |s| serde_json::to_string(s).unwrap());
            black_box(arc.len())
        })
    });
}

criterion_group!(
    benches,
    bench_naive_serialize_60_tools,
    bench_cached_serialize_60_tools
);
criterion_main!(benches);
