// Benchmarks the vector-search code path: dot-product hot loop with
// pre-computed norm versus naive per-row norm computation.
//
// Run: `cargo bench --bench vector_search`

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn naive_cosine(a: &[f32], b: &[f32]) -> f32 {
    let na = norm(a);
    let nb = norm(b);
    if na < f32::EPSILON || nb < f32::EPSILON {
        return 0.0;
    }
    dot_product(a, b) / (na * nb)
}

fn cached_cosine(a: &[f32], a_norm: f32, b: &[f32], b_norm: f32) -> f32 {
    if a_norm < f32::EPSILON || b_norm < f32::EPSILON {
        return 0.0;
    }
    dot_product(a, b) / (a_norm * b_norm)
}

fn random_vec(seed: u64, dim: usize) -> Vec<f32> {
    let mut s = seed;
    (0..dim)
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let x = (s >> 32) as u32;
            (x as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

fn bench_vector_search_10k(c: &mut Criterion) {
    let dim = 384;
    let n = 10_000;
    let query = random_vec(0, dim);
    let query_norm = norm(&query);
    let corpus: Vec<Vec<f32>> = (1..=n).map(|s| random_vec(s as u64, dim)).collect();
    let corpus_norms: Vec<f32> = corpus.iter().map(|v| norm(v)).collect();

    c.bench_function("vector_search_naive_10k_d384", |b| {
        b.iter(|| {
            let mut best = 0.0_f32;
            for v in &corpus {
                let s = naive_cosine(&query, v);
                if s > best {
                    best = s;
                }
            }
            black_box(best)
        })
    });

    c.bench_function("vector_search_cached_10k_d384", |b| {
        b.iter(|| {
            let mut best = 0.0_f32;
            for (v, vn) in corpus.iter().zip(corpus_norms.iter()) {
                let s = cached_cosine(&query, query_norm, v, *vn);
                if s > best {
                    best = s;
                }
            }
            black_box(best)
        })
    });
}

criterion_group!(benches, bench_vector_search_10k);
criterion_main!(benches);
