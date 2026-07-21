// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};

use super::symbol_graph::{SymbolGraph, SymbolId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub commit: Option<String>,
    pub author: Option<String>,
    pub author_time_unix: Option<i64>,
    pub summary: Option<String>,
}

impl TimelineEntry {
    pub const fn empty() -> Self {
        Self {
            commit: None,
            author: None,
            author_time_unix: None,
            summary: None,
        }
    }
}

pub fn build_timeline(root: &Path, graph: &SymbolGraph) -> HashMap<SymbolId, TimelineEntry> {
    build_timeline_scoped(root, graph, DEFAULT_MAX_BLAME_FILES)
}

/// Upper bound on how many files `recent_changes` will blame in one call. A blame
/// is one git subprocess per file; blaming an entire large repo (tens of
/// thousands of files) previously spawned tens of thousands of processes and took
/// minutes. We restrict to the files touched by recent commits, capped here.
const DEFAULT_MAX_BLAME_FILES: usize = 400;

/// Build the timeline but only blame files that appear in recent git history,
/// capped at `max_files`. Symbols in unblamed files get an empty entry (they are
/// not "recently changed", which is exactly what this query asks for).
pub fn build_timeline_scoped(
    root: &Path,
    graph: &SymbolGraph,
    max_files: usize,
) -> HashMap<SymbolId, TimelineEntry> {
    let mut out: HashMap<SymbolId, TimelineEntry> = HashMap::new();
    if !is_git_repo(root) {
        for entry in &graph.symbols {
            out.insert(entry.id.clone(), TimelineEntry::empty());
        }
        return out;
    }

    let recent = recently_changed_files(root, max_files);

    let mut by_file: HashMap<&std::path::Path, Vec<&SymbolId>> = HashMap::new();
    for entry in &graph.symbols {
        by_file
            .entry(entry.id.file.as_path())
            .or_default()
            .push(&entry.id);
    }

    for (file, syms) in by_file {
        // Only blame files git says changed recently; everything else gets an
        // empty entry without spawning a subprocess.
        let should_blame = recent.is_empty()
            || recent.contains(&normalize_rel(file));
        let blame = if should_blame {
            let lines_of_interest: Vec<u32> = syms.iter().map(|s| s.line).collect();
            run_blame(root, file, &lines_of_interest).unwrap_or_default()
        } else {
            HashMap::new()
        };
        for sym in syms {
            let entry = blame
                .get(&sym.line)
                .cloned()
                .unwrap_or_else(TimelineEntry::empty);
            out.insert(sym.clone(), entry);
        }
    }
    out
}

fn normalize_rel(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Returns the set of files (workspace-relative, `/`-separated) touched by recent
/// commits, capped at `max_files`. Uses a single `git log --name-only` instead of
/// per-file blame so the caller can prune the blame set up front.
fn recently_changed_files(root: &Path, max_files: usize) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let out = crate::util::hidden_sync_command("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "--name-only",
            "--pretty=format:",
            "-n",
            "400",
            "--since=180.days",
        ])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                set.insert(l.replace('\\', "/"));
                if set.len() >= max_files {
                    break;
                }
            }
        }
    }
    set
}

fn is_git_repo(root: &Path) -> bool {
    let out = crate::util::hidden_sync_command("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();
    match out {
        Ok(o) => o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true",
        Err(_) => false,
    }
}

fn run_blame(root: &Path, file: &Path, lines: &[u32]) -> Option<HashMap<u32, TimelineEntry>> {
    if lines.is_empty() {
        return Some(HashMap::new());
    }
    let file_str = file.to_string_lossy().to_string();
    let mut cmd = crate::util::hidden_sync_command("git");
    cmd.arg("-C").arg(root).arg("blame").arg("--porcelain");
    for line in lines {
        cmd.arg("-L").arg(format!("{line},{line}"));
    }
    cmd.arg("--").arg(&file_str);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(parse_blame_porcelain(&stdout))
}

pub(crate) fn parse_blame_porcelain(out: &str) -> HashMap<u32, TimelineEntry> {
    let mut result: HashMap<u32, TimelineEntry> = HashMap::new();
    let mut commits: HashMap<String, TimelineEntry> = HashMap::new();

    let mut current_hash: Option<String> = None;
    let mut current_line: Option<u32> = None;
    let mut current_entry = TimelineEntry::empty();
    let mut in_block = false;

    for raw in out.lines() {

        if raw.len() >= 41 && raw.as_bytes()[40] == b' ' {

            finalise_block(
                &mut result,
                &mut commits,
                &current_hash,
                current_line,
                &current_entry,
            );
            current_entry = TimelineEntry::empty();

            let hash = raw[..40].to_string();

            let rest = &raw[41..];
            let mut it = rest.split_whitespace();
            let _orig = it.next();
            let final_line = it.next().and_then(|s| s.parse::<u32>().ok());
            current_hash = Some(hash.clone());
            current_line = final_line;

            if let Some(cached) = commits.get(&hash) {
                current_entry = cached.clone();
                current_entry.commit = Some(hash);
            } else {
                current_entry.commit = Some(hash);
            }
            in_block = true;
            continue;
        }
        if !in_block {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("author ") {
            current_entry.author = Some(rest.to_string());
        } else if let Some(rest) = raw.strip_prefix("author-time ") {
            current_entry.author_time_unix = rest.parse::<i64>().ok();
        } else if let Some(rest) = raw.strip_prefix("summary ") {
            current_entry.summary = Some(rest.to_string());
        } else if raw.starts_with('\t') {

            finalise_block(
                &mut result,
                &mut commits,
                &current_hash,
                current_line,
                &current_entry,
            );
            in_block = false;
            current_line = None;
            current_hash = None;
            current_entry = TimelineEntry::empty();
        }
    }

    if in_block {
        finalise_block(
            &mut result,
            &mut commits,
            &current_hash,
            current_line,
            &current_entry,
        );
    }
    result
}

fn finalise_block(
    result: &mut HashMap<u32, TimelineEntry>,
    commits: &mut HashMap<String, TimelineEntry>,
    hash: &Option<String>,
    line: Option<u32>,
    entry: &TimelineEntry,
) {
    if let (Some(h), Some(l)) = (hash, line) {

        commits.entry(h.clone()).or_insert_with(|| entry.clone());
        result.insert(l, entry.clone());
    }
}
