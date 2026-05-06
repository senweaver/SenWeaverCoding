// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Phase 4 D1.1 — multi-writer contention bench for the Blackboard /
//! ShardedMap.  Compares 16-shard vs 1-shard concurrent write
//! throughput on 8 threads × 2000 disjoint keys.  The sharded layout
//! should deliver ≥ 3× throughput on any multi-core host.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use std::thread;

use senweavercoding::memory::sharded_map::ShardedMap;

const THREADS: usize = 8;
const KEYS_PER_THREAD: usize = 2000;

fn run_writers(map: Arc<ShardedMap<u64>>) {
    let mut handles = Vec::with_capacity(THREADS);
    for t in 0..THREADS {
        let map = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for i in 0..KEYS_PER_THREAD {
                let k = format!("t{t}-k{i}");
                map.insert(k, (t as u64) * 1_000_000 + i as u64);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}

fn bench_sharded_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("blackboard_ops_parallel_writers");
    group.throughput(Throughput::Elements((THREADS * KEYS_PER_THREAD) as u64));

    for shards in [1usize, 4, 16, 64] {
        group.bench_with_input(BenchmarkId::new("shards", shards), &shards, |b, &shards| {
            b.iter_with_setup(
                || Arc::new(ShardedMap::<u64>::with_shards(shards)),
                |map| {
                    run_writers(black_box(map));
                },
            );
        });
    }

    group.finish();
}

fn bench_single_shard_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("blackboard_ops_hot_key_reads");
    let map: ShardedMap<u64> = ShardedMap::new();
    map.insert("hot-key".into(), 42);

    group.bench_function("get_cloned_1m", |b| {
        b.iter(|| {
            for _ in 0..1_000_000 {
                black_box(map.get_cloned("hot-key"));
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_sharded_writes, bench_single_shard_reads);
criterion_main!(benches);
