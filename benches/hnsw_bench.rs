// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use senweavercoding::memory::hnsw::{HnswMemIndex, HnswParams};
use senweavercoding::memory::vector_index::VectorIndex;

fn rand_vec(seed: u64, dim: usize) -> Vec<f32> {
    let mut s = seed;
    (0..dim)
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let x = (s >> 32) as u32;
            ((x as f32) / (u32::MAX as f32)) * 2.0 - 1.0
        })
        .collect()
}

fn build_index(n: usize, dim: usize) -> HnswMemIndex {
    let mut idx = HnswMemIndex::with_params(HnswParams::default());
    for i in 0..n {
        let v = rand_vec(i as u64 + 1, dim);
        idx.upsert(&format!("doc-{i}"), &v);
    }
    idx
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("hnsw_insert");
    let dim = 128;
    for &n in &[1_000usize, 5_000, 20_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let base = build_index(n, dim);
            b.iter_batched(
                || base.clone(),
                |mut idx: HnswMemIndex| {
                    let v = rand_vec(n as u64 + 999, dim);
                    idx.upsert("probe", black_box(&v));
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("hnsw_search");
    let dim = 128;
    for &n in &[1_000usize, 5_000, 20_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let idx = build_index(n, dim);
            let q = rand_vec(42, dim);
            b.iter(|| {
                let hits = idx.search(black_box(&q), 10);
                black_box(hits);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_insert, bench_search);
criterion_main!(benches);
