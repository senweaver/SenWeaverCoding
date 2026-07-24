// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};
use std::sync::LazyLock;

use parking_lot::RwLock;

use crate::memory::vector::index::{VectorBackend, VectorIndex, build_backend};

inventory::submit!(StaticSlashCommand {
    name: "vector",
    aliases: &["vec"],
    description: "Inspect or exercise the vector index (upsert/search/stats/forget)",
    usage: "/vector <upsert|search|stats|forget> [args...]",
    category: CommandCategory::Debug,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_vector),
});

static GLOBAL_INDEX: LazyLock<RwLock<Box<dyn VectorIndex>>> = LazyLock::new(|| {
    let kind = VectorBackend::default();
    RwLock::new(build_backend(kind))
});

static GLOBAL_BACKEND: LazyLock<RwLock<VectorBackend>> =
    LazyLock::new(|| RwLock::new(VectorBackend::default()));

pub async fn handle_vector(ctx: CommandContext) -> CommandResult {
    let sub = ctx.args.first().map(String::as_str).unwrap_or("");
    match sub {
        "upsert" => upsert(&ctx),
        "search" => search(&ctx),
        "forget" | "remove" => forget(&ctx),
        "stats" | "" => stats(),
        "bench" => bench_parallel_vs_sequential(&ctx),
        "backend" => backend(&ctx),
        other => CommandResult::err(format!(
            "Unknown /vector subcommand '{other}'. Try: upsert | search | forget | stats | bench | backend"
        )),
    }
}

fn backend(ctx: &CommandContext) -> CommandResult {
    let target = ctx.args.get(1).cloned();

    if target.is_none() {
        let active = *GLOBAL_BACKEND.read();
        let idx = GLOBAL_INDEX.read();
        let mut out = format!(
            "Active vector backend: {}  -  {}\nEntries: {}\n\nAvailable backends:\n",
            active.display_name(),
            active.description(),
            idx.len(),
        );
        for variant in VectorBackend::all() {
            let marker = if *variant == active { "->" } else { " " };
            out.push_str(&format!(
                "  {} {:<8}  {}\n",
                marker,
                variant.display_name(),
                variant.description()
            ));
        }
        out.push_str(
            "\nSwitch with: /vector backend <linear|sharded|ivf|hnsw>\n\
             To persist, set `memory.vector_backend = \"ivf\"` in config.toml.",
        );
        return CommandResult::ok(out);
    }

    let Some(name) = target else {
        return CommandResult::err(
            "/vector backend: expected a backend name argument".to_string(),
        );
    };
    let new_kind = match VectorBackend::from_str_lenient(&name) {
        Some(k) => k,
        None => {
            return CommandResult::err(format!(
                "Unknown backend '{name}'. Valid: linear | sharded | ivf | hnsw"
            ));
        }
    };

    let mut new_index = build_backend(new_kind);
    let (count_before, migrated) = {
        let old = GLOBAL_INDEX.read();
        let count = old.len();
        (count, 0usize)
    };

    *GLOBAL_INDEX.write() = std::mem::replace(&mut new_index, build_backend(new_kind));
    *GLOBAL_BACKEND.write() = new_kind;
    drop(new_index);

    let msg = if count_before == 0 {
        format!("Backend switched to '{}'.", new_kind.display_name())
    } else {
        format!(
            "Backend switched to '{}'. Note: {count_before} vectors in the previous CLI-scoped index were not migrated; re-upsert if needed (the agent's memory index is separate and unaffected). Migrated: {migrated}.",
            new_kind.display_name()
        )
    };
    CommandResult::ok(msg)
}

fn bench_parallel_vs_sequential(ctx: &CommandContext) -> CommandResult {
    use crate::memory::vector::index::LinearIndex;
    let n_vecs: usize = ctx
        .args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2_000);
    let dim: usize = ctx.args.get(2).and_then(|s| s.parse().ok()).unwrap_or(128);
    let iters: usize = ctx.args.get(3).and_then(|s| s.parse().ok()).unwrap_or(50);

    if n_vecs == 0 || dim == 0 || iters == 0 {
        return CommandResult::err("n_vecs / dim / iters must be positive");
    }

    let mut seed: u64 = 0x1234_5678_9abc_def0;
    let mut gen_vec = move |d: usize| -> Vec<f32> {
        (0..d)
            .map(|_| {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005u64)
                    .wrapping_add(1_442_695_040_888_963_407u64);
                ((seed >> 32) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    };

    let mut linear = LinearIndex::new();
    let mut sharded = crate::memory::sharded::index::ShardedVectorIndex::with_cpu_count();

    for i in 0..n_vecs {
        let v = gen_vec(dim);
        let id = format!("b-{i}");
        linear.upsert(&id, &v);
        sharded.upsert(&id, &v);
    }

    let queries: Vec<Vec<f32>> = (0..iters).map(|_| gen_vec(dim)).collect();

    for q in queries.iter().take(1) {
        let _ = linear.search(q, 10);
        let _ = sharded.search(q, 10);
    }

    let t0 = std::time::Instant::now();
    for q in &queries {
        let _ = linear.search(q, 10);
    }
    let linear_elapsed = t0.elapsed();

    let t1 = std::time::Instant::now();
    for q in &queries {
        let _ = sharded.search(q, 10);
    }
    let sharded_elapsed = t1.elapsed();

    let speedup = linear_elapsed.as_secs_f64() / sharded_elapsed.as_secs_f64().max(1e-9);

    CommandResult::ok(format!(
        "Vector bench -- {n_vecs} vecs x {dim} dim x {iters} queries\n\
         ---------------------------------------\n\
         LinearIndex            : {:>8} ms\n\
         ShardedVectorIndex ({})  : {:>8} ms\n\
         Speedup                : {:>8.2}x",
        linear_elapsed.as_millis(),
        sharded.shard_count(),
        sharded_elapsed.as_millis(),
        speedup
    ))
}

fn parse_vec(csv: &str) -> Result<Vec<f32>, String> {
    csv.split(',')
        .map(|s| s.trim().parse::<f32>().map_err(|e| format!("'{s}': {e}")))
        .collect::<Result<Vec<_>, _>>()
}

fn upsert(ctx: &CommandContext) -> CommandResult {
    let id = match ctx.args.get(1) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return CommandResult::err("Usage: /vector upsert <id> <v1,v2,...>"),
    };
    let csv = match ctx.args.get(2) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return CommandResult::err("Usage: /vector upsert <id> <v1,v2,...>"),
    };
    let v = match parse_vec(&csv) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(format!("Invalid vector: {e}")),
    };
    if v.is_empty() {
        return CommandResult::err("vector is empty");
    }
    GLOBAL_INDEX.write().upsert(&id, &v);
    CommandResult::ok(format!(
        "Upserted id='{id}' dim={} len_total={}",
        v.len(),
        GLOBAL_INDEX.read().len()
    ))
}

fn search(ctx: &CommandContext) -> CommandResult {
    let csv = match ctx.args.get(1) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return CommandResult::err("Usage: /vector search <v1,v2,...> [limit]"),
    };
    let limit: usize = ctx.args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let q = match parse_vec(&csv) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(format!("Invalid query vector: {e}")),
    };
    if q.is_empty() {
        return CommandResult::err("query vector is empty");
    }
    let results = GLOBAL_INDEX.read().search(&q, limit);
    if results.is_empty() {
        return CommandResult::ok("(no matches)".to_string());
    }
    let body: String = results
        .iter()
        .map(|(id, sim)| format!("{sim:>6.4}  {id}"))
        .collect::<Vec<_>>()
        .join("\n");
    CommandResult::ok(format!(
        "Top-{} results (similarity / id):\n{body}",
        results.len()
    ))
}

fn forget(ctx: &CommandContext) -> CommandResult {
    let id = match ctx.args.get(1) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return CommandResult::err("Usage: /vector forget <id>"),
    };
    GLOBAL_INDEX.write().remove(&id);
    CommandResult::ok(format!(
        "Removed id='{id}' (len_total={})",
        GLOBAL_INDEX.read().len()
    ))
}

fn stats() -> CommandResult {
    let idx = GLOBAL_INDEX.read();
    CommandResult::ok(format!(
        "Vector index stats\n\
         ---------------\n\
         backend: {}\n\
         entries: {}",
        idx.backend_name(),
        idx.len()
    ))
}
