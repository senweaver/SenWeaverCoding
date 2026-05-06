// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Benchmarks for N1-v2 `turn_streamed` deep optimisations.
//!
//! What we measure:
//!   * `event_canonicalize` — JSON hashing cost of a typical scenario
//!     (used by `tests/turn_event_stability.rs` for the golden hash
//!     protocol).  Should stay well under 10µs per 20-event sequence.
//!   * `event_fanout` — simulated cost of one turn worth of N1-v2
//!     TurnEvent fanout through in-memory translators.  Establishes
//!     a floor so future regressions in the event pipeline show up.
//!
//! We deliberately **don't** spin up a real Agent here.  A real
//! end-to-end bench needs network mocks, config bootstrap, and a
//! ~1 second warmup that swamps the actual streaming cost.  Those
//! concerns belong in a separate `benches/agent_turn.rs` (TODO).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use senweavercoding::agent::TurnEvent;

/// Build a scenario with `n` streamed text chunks + 1 tool call +
/// 1 file edit + 1 tool result, for scale testing.
fn build_scenario(n_chunks: usize) -> Vec<TurnEvent> {
    let mut out = Vec::with_capacity(n_chunks + 6);
    out.push(TurnEvent::ProgressTick {
        iteration: 0,
        max_iterations: 50,
        tokens_used: 0,
    });
    for i in 0..n_chunks {
        out.push(TurnEvent::Chunk {
            delta: format!("chunk-{i} "),
        });
    }
    out.push(TurnEvent::ToolCall {
        name: "file_write".into(),
        args: serde_json::json!({"path": "src/lib.rs", "content": "..."}),
    });
    out.push(TurnEvent::ToolResult {
        name: "file_write".into(),
        output: r#"{"path": "src/lib.rs", "bytes": 512}"#.into(),
    });
    out.push(TurnEvent::FileEdit {
        path: "src/lib.rs".into(),
        additions: 12,
        deletions: 3,
        diff: None,
        edit_batch_id: None,
    });
    out.push(TurnEvent::ContextCompressed {
        tokens_before: 90_000,
        tokens_after: 40_000,
    });
    out.push(TurnEvent::Chunk {
        delta: "Done.".into(),
    });
    out
}

fn canonicalize(events: &[TurnEvent]) -> String {
    let mut values: Vec<serde_json::Value> = Vec::with_capacity(events.len());
    for ev in events {
        values.push(event_to_json(ev));
    }
    serde_json::to_string(&values).unwrap()
}

fn event_to_json(event: &TurnEvent) -> serde_json::Value {
    use serde_json::json;
    match event {
        TurnEvent::Chunk { delta } => json!({ "kind": "Chunk", "delta": delta }),
        TurnEvent::Thinking { delta } => json!({ "kind": "Thinking", "delta": delta }),
        TurnEvent::ToolCall { name, args } => {
            json!({ "kind": "ToolCall", "name": name, "args": args })
        }
        TurnEvent::ToolResult { name, output } => {
            json!({ "kind": "ToolResult", "name": name, "output": output })
        }
        TurnEvent::Error { message } => json!({ "kind": "Error", "message": message }),
        TurnEvent::FileEdit {
            path,
            additions,
            deletions,
            diff,
            ..
        } => json!({
            "kind": "FileEdit", "path": path,
            "additions": additions, "deletions": deletions, "diff": diff,
        }),
        TurnEvent::StatusUpdate { action, detail } => {
            json!({ "kind": "StatusUpdate", "action": action, "detail": detail })
        }
        TurnEvent::ProgressTick {
            iteration,
            max_iterations,
            tokens_used,
        } => json!({
            "kind": "ProgressTick",
            "iteration": iteration,
            "max_iterations": max_iterations,
            "tokens_used": tokens_used,
        }),
        TurnEvent::CommandPreview {
            tool_name,
            args,
            estimated_duration_ms,
        } => json!({
            "kind": "CommandPreview",
            "tool_name": tool_name,
            "args": args,
            "estimated_duration_ms": estimated_duration_ms,
        }),
        TurnEvent::Cancelling { reason } => json!({ "kind": "Cancelling", "reason": reason }),
        TurnEvent::ContextCompressed {
            tokens_before,
            tokens_after,
        } => json!({
            "kind": "ContextCompressed",
            "tokens_before": tokens_before,
            "tokens_after": tokens_after,
        }),
        TurnEvent::SubagentChunk {
            task_id,
            agent_id,
            kind,
            delta,
        } => json!({
            "kind": "SubagentChunk",
            "task_id": task_id,
            "agent_id": agent_id,
            "subkind": format!("{kind:?}"),
            "delta": delta,
        }),
    }
}

fn bench_event_canonicalize(c: &mut Criterion) {
    let mut group = c.benchmark_group("turn_event_canonicalize");
    for size in [10usize, 50, 100, 200] {
        let events = build_scenario(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let json = canonicalize(&events);
                std::hint::black_box(json);
            });
        });
    }
    group.finish();
}

fn bench_event_hash(c: &mut Criterion) {
    let events = build_scenario(50);
    c.bench_function("turn_event_sha256_hash", |b| {
        b.iter(|| {
            use sha2::{Digest, Sha256};
            let json = canonicalize(&events);
            let mut hasher = Sha256::new();
            hasher.update(json.as_bytes());
            let out = hex::encode(hasher.finalize());
            std::hint::black_box(out);
        });
    });
}

fn bench_event_clone(c: &mut Criterion) {
    // Proxy for the cost of sending a TurnEvent across an
    // `mpsc::Sender`; clone dominates that cost.
    let events = build_scenario(50);
    c.bench_function("turn_event_clone_50", |b| {
        b.iter(|| {
            let cloned: Vec<TurnEvent> = events.clone();
            std::hint::black_box(cloned);
        });
    });
}

// ── tool_specs clone cost comparison ───────────────────────────────────────

/// Generate a realistic ToolSpec payload with non-trivial JSON args.
fn make_tool_specs(n: usize) -> Vec<senweavercoding::tools::ToolSpec> {
    (0..n)
        .map(|i| senweavercoding::tools::ToolSpec {
            name: format!("tool_{i}"),
            description: "A tool with a somewhat long description to increase struct size and measure clone cost".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "options": {
                        "type": "object",
                        "properties": {
                            "recursive": { "type": "boolean" },
                            "encoding": { "type": "string" }
                        }
                    }
                }
            }),
        })
        .collect()
}

/// Arc-clone: O(1) — only the pointer is cloned.
fn bench_tool_specs_arc_clone(c: &mut Criterion) {
    let specs = make_tool_specs(50);
    let arc = std::sync::Arc::new(specs);
    c.bench_function("tool_specs_arc_clone_50", |b| {
        b.iter(|| {
            let cloned = arc.clone();
            std::hint::black_box(cloned);
        });
    });
}

/// Vec-clone: O(n) — the entire vec and all elements are copied.
fn bench_tool_specs_vec_clone(c: &mut Criterion) {
    let specs = make_tool_specs(50);
    c.bench_function("tool_specs_vec_clone_50", |b| {
        b.iter(|| {
            let cloned: Vec<_> = specs.clone();
            std::hint::black_box(cloned);
        });
    });
}

/// Arc-clone with owned-conversion (the pattern used in loop_.rs when activated_tools exist).
fn bench_tool_specs_arc_to_owned(c: &mut Criterion) {
    let specs = make_tool_specs(50);
    let arc = std::sync::Arc::new(specs);
    c.bench_function("tool_specs_arc_to_owned_50", |b| {
        b.iter(|| {
            // This matches the code in loop_.rs: Arc clone + deref + owned clone
            let owned: Vec<_> = (*arc).clone();
            std::hint::black_box(owned);
        });
    });
}

fn bench_tool_specs_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("tool_specs_clone");
    for n in [10usize, 50, 100, 200] {
        let specs = make_tool_specs(n);
        let arc = std::sync::Arc::new(specs.clone());
        group.bench_with_input(BenchmarkId::new("Arc", n), &n, |b, _| {
            b.iter(|| {
                let _cloned = arc.clone();
                std::hint::black_box(());
            });
        });
        group.bench_with_input(BenchmarkId::new("Vec", n), &n, |b, _| {
            b.iter(|| {
                let _cloned: Vec<_> = specs.clone();
                std::hint::black_box(());
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_event_canonicalize,
    bench_event_hash,
    bench_event_clone,
    bench_tool_specs_arc_clone,
    bench_tool_specs_vec_clone,
    bench_tool_specs_arc_to_owned,
    bench_tool_specs_scaling,
);
criterion_main!(benches);
