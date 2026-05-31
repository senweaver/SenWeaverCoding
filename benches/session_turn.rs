// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
