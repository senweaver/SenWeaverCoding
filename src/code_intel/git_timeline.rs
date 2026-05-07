// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! M6-Git timeline.
//!
//! Associates every symbol in a [`super::SymbolGraph`] with the most
//! recent commit / author that touched its declaration line, using
//! the system `git` CLI.  The module deliberately shells out rather
//! than linking libgit2 unconditionally so the feature works on
//! every build (the existing `git2` dependency is gated behind the
//! `gui` feature).  When `git` is unavailable or the workspace is
//! not a repo, the timeline degrades to `None` entries — callers
//! must treat "no data" as a first-class state rather than an
//! error.

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
    let mut out: HashMap<SymbolId, TimelineEntry> = HashMap::new();
    if !is_git_repo(root) {
        for entry in &graph.symbols {
            out.insert(entry.id.clone(), TimelineEntry::empty());
        }
        return out;
    }

    let mut by_file: HashMap<&std::path::Path, Vec<&SymbolId>> = HashMap::new();
    for entry in &graph.symbols {
        by_file
            .entry(entry.id.file.as_path())
            .or_default()
            .push(&entry.id);
    }

    for (file, syms) in by_file {
        let lines_of_interest: Vec<u32> = syms.iter().map(|s| s.line).collect();
        let blame = run_blame(root, file, &lines_of_interest).unwrap_or_default();
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
