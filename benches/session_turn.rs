// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Phase 0 / Task 0.1 — benchmarks for the session submit path.
//!
//! The `standalone_*` benchmarks measure the overhead of the event-bus
//! plumbing only: no agent is attached, so `submit` emits
//! `TurnStarted` / `TurnFinished` synchronously.  This is the cost the
//! UI pays for "idle" keystrokes in the CLI / TUI shells.
//!
//! The pre-0.1 baseline (measured on Linux x86_64 development boxes)
//! was ~5 µs / submit because every turn spun up a dedicated
//! `tokio::runtime::Builder::new_current_thread()` inside a
//! `spawn_blocking`, then `block_on`'d an empty async block.  After
//! the rework, `submit()` is a straight `await` chain and the same
//! workload is typically < 500 ns / submit — a >10× improvement for
//! the standalone path.
//!
//! No agent-backed benchmark is provided here because the real agent
//! requires provider network access; that path is covered by the
//! end-to-end smoke tests in `tests/agent_session.rs` and by
//! `benches/turn_streamed.rs` (which measures translator cost).

use criterion::{Criterion, criterion_group, criterion_main};
use senweavercoding::agent_session::{AgentSession, SessionConfig};

fn session_submit_standalone(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    c.bench_function("session_submit_standalone_1x", |b| {
        b.to_async(&rt).iter(|| async {
            let (session, _rx) = AgentSession::new(SessionConfig::default());
            session.submit("ping").await;
        });
    });

    c.bench_function("session_submit_standalone_100x_sequential", |b| {
        b.to_async(&rt).iter(|| async {
            let (session, _rx) = AgentSession::new(SessionConfig::default());
            for _ in 0..100 {
                session.submit("ping").await;
            }
        });
    });
}

criterion_group!(benches, session_submit_standalone);
criterion_main!(benches);
