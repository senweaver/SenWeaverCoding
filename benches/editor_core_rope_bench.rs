// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Benchmarks for editor_core performance — rope vs legacy Vec<String>.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use senweavercoding::editor_core::buffer::{Position, Selection, TextBuffer};
use senweavercoding::editor_core::search::{SearchOptions, find_all, replace_all};
use std::hint::black_box;

// Generate a large text buffer for benchmarking (100KB of typical source code).
fn gen_source_code(size_kb: usize) -> String {
    let line = "fn process_item(id: u32, name: &str) -> Result<Box<dyn Any>, ProcessError> {\n    let data = fetch_data(id).await?;\n    validate_input(&data)?;\n    // TODO: optimize the inner loop\n    for item in data.iter() {\n        if item.is_valid() {\n            handle_valid(item).await;\n        } else {\n            log::warn(\"invalid item: {:?}\", item);\n        }\n    }\n    Ok(Box::new(data))\n}\n";
    let bytes_per_line = line.len();
    let num_lines = (size_kb * 1024) / bytes_per_line;
    line.repeat(num_lines.max(1))
}

// --- TextBuffer benchmarks ---

fn bench_from_str(c: &mut Criterion) {
    let text = gen_source_code(100);
    c.bench_function("editor_core/from_str_100kb", |b| {
        b.iter(|| TextBuffer::from_str(black_box(&text)));
    });
}

fn bench_insert_middle(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    let pos = Position { line: 500, col: 30 };
    c.bench_function("editor_core/insert_middle_100kb", |b| {
        let mut buf = buf.clone();
        b.iter(|| {
            buf.insert(black_box(pos), black_box(" INSERTED "));
        });
    });
}

fn bench_insert_end(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    let last_line = buf.line_count() - 1;
    let pos = Position {
        line: last_line,
        col: 0,
    };
    c.bench_function("editor_core/insert_end_100kb", |b| {
        let mut buf = buf.clone();
        b.iter(|| {
            buf.insert(black_box(pos), black_box(" APPENDED "));
        });
    });
}

fn bench_delete_range(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    let sel = Selection {
        start: Position { line: 100, col: 10 },
        end: Position { line: 200, col: 30 },
    };
    c.bench_function("editor_core/delete_range_100kb", |b| {
        let mut buf = buf.clone();
        b.iter(|| {
            buf.delete(black_box(&sel));
        });
    });
}

fn bench_line_count(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/line_count_100kb", |b| {
        b.iter(|| black_box(buf.line_count()));
    });
}

fn bench_line_access(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/line_access_100kb", |b| {
        b.iter(|| {
            for line_idx in 0..buf.line_count() {
                black_box(buf.line(line_idx));
            }
        });
    });
}

fn bench_char_count(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/char_count_100kb", |b| {
        b.iter(|| black_box(buf.char_count()));
    });
}

fn bench_char_idx_roundtrip(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    let pos = Position { line: 500, col: 40 };
    c.bench_function("editor_core/char_idx_roundtrip_100kb", |b| {
        b.iter(|| {
            let idx = buf.char_idx(black_box(pos));
            black_box(buf.idx_to_position(idx));
        });
    });
}

fn bench_to_string(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/to_string_100kb", |b| {
        b.iter(|| black_box(buf.to_string()));
    });
}

fn bench_clone(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/clone_100kb", |b| {
        b.iter(|| black_box(buf.clone()));
    });
}

// --- Search benchmarks ---

fn bench_search_literal(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/search_literal_100kb", |b| {
        b.iter(|| black_box(find_all(&buf, black_box("item"), &SearchOptions::default())));
    });
}

fn bench_search_case_insensitive(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/search_ci_100kb", |b| {
        b.iter(|| {
            black_box(find_all(
                &buf,
                black_box("Item"),
                &SearchOptions {
                    case_insensitive: true,
                    whole_word: false,
                },
            ))
        });
    });
}

fn bench_search_whole_word(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/search_whole_word_100kb", |b| {
        b.iter(|| {
            black_box(find_all(
                &buf,
                black_box("fn"),
                &SearchOptions {
                    case_insensitive: false,
                    whole_word: true,
                },
            ))
        });
    });
}

fn bench_replace_all(c: &mut Criterion) {
    let text = gen_source_code(100);
    let buf = TextBuffer::from_str(&text);
    c.bench_function("editor_core/replace_all_100kb", |b| {
        b.iter(|| {
            black_box(replace_all(
                &buf,
                black_box("fn"),
                black_box("func"),
                &SearchOptions::default(),
            ))
        });
    });
}

// --- Scaling benchmarks ---

fn bench_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_core/scaling");
    for size_kb in [10, 50, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(size_kb),
            &size_kb,
            |b, &size| {
                let text = gen_source_code(size);
                b.iter(|| {
                    let mut buf = TextBuffer::from_str(black_box(&text));
                    buf.insert(Position { line: 0, col: 0 }, black_box("x"));
                    black_box(buf.char_count())
                });
            },
        );
    }
    group.finish();
}

fn bench_search_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_core/search_scaling");
    for size_kb in [10, 50, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(size_kb),
            &size_kb,
            |b, &size| {
                let text = gen_source_code(size);
                let buf = TextBuffer::from_str(&text);
                b.iter(|| black_box(find_all(&buf, black_box("item"), &SearchOptions::default())));
            },
        );
    }
    group.finish();
}

criterion_group!(
    editor_core_benches,
    bench_from_str,
    bench_insert_middle,
    bench_insert_end,
    bench_delete_range,
    bench_line_count,
    bench_line_access,
    bench_char_count,
    bench_char_idx_roundtrip,
    bench_to_string,
    bench_clone,
    bench_search_literal,
    bench_search_case_insensitive,
    bench_search_whole_word,
    bench_replace_all,
    bench_scaling,
    bench_search_scaling,
);
criterion_main!(editor_core_benches);
