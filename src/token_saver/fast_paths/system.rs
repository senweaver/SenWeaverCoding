// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::token_saver::pipeline;
use crate::token_saver::{CompactContext, CompactLevel, DirEntry, GrepHit, GrepOpts, ListOpts};
use std::collections::BTreeMap;

pub fn ls(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    if matches!(ctx.level, CompactLevel::Conservative) {
        return (scrub, stderr);
    }

    let mut groups: BTreeMap<String, u32> = BTreeMap::new();
    let mut others: Vec<String> = Vec::new();
    for line in scrub.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(dot) = trimmed.rfind('.') {
            let ext = &trimmed[dot + 1..];
            if !ext.is_empty() && ext.chars().all(|c| c.is_alphanumeric()) {
                *groups.entry(ext.to_string()).or_default() += 1;
                continue;
            }
        }
        others.push(trimmed.to_string());
    }
    let mut out = String::new();
    for o in others {
        out.push_str(&o);
        out.push('\n');
    }
    for (ext, n) in groups {
        out.push_str(&format!("*.{ext} ({n})\n"));
    }
    (out, stderr)
}

pub fn find(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    if matches!(ctx.level, CompactLevel::Conservative) {
        return (scrub, stderr);
    }

    let mut count = 0u32;
    let mut total = 0u32;
    let mut out = String::with_capacity(scrub.len() / 2);
    for line in scrub.lines() {
        total += 1;
        if count < 200 {
            out.push_str(line);
            out.push('\n');
            count += 1;
        }
    }
    if total > 200 {
        out.push_str(&format!("... [{} more results truncated]\n", total - 200));
    }
    (out, stderr)
}

pub fn grep(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    if matches!(ctx.level, CompactLevel::Conservative) {
        return (scrub, stderr);
    }

    let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in scrub.lines() {
        if let Some((file, _rest)) = line.split_once(':') {
            let entry = by_file.entry(file.to_string()).or_default();
            if !order.contains(&file.to_string()) {
                order.push(file.to_string());
            }
            entry.push(line.to_string());
        } else {
            order.push(line.to_string());
            by_file.entry(line.to_string()).or_default().push(line.to_string());
        }
    }
    let mut out = String::new();
    for file in order {
        if let Some(hits) = by_file.get(&file) {
            let n = hits.len();
            if n == 0 {
                continue;
            }
            for h in hits.iter().take(5) {
                out.push_str(h);
                out.push('\n');
            }
            if n > 5 {
                out.push_str(&format!("    +{} more matches in {}\n", n - 5, file));
            }
        }
    }
    (out, stderr)
}

pub fn cat(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    if matches!(ctx.level, CompactLevel::Conservative) {
        return (scrub, stderr);
    }

    let lines: Vec<&str> = scrub.lines().collect();
    if lines.len() <= 200 {
        return (scrub, stderr);
    }
    let mut out = String::new();
    for l in lines.iter().take(120) {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(&format!(
        "... [{} lines elided] ...\n",
        lines.len() - 120 - 60
    ));
    for l in lines.iter().skip(lines.len() - 60) {
        out.push_str(l);
        out.push('\n');
    }
    (out, stderr)
}

pub fn compact_grep(matches: &[GrepHit], opts: &GrepOpts) -> String {
    let mut by_file: BTreeMap<String, Vec<&GrepHit>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for hit in matches {
        if !order.contains(&hit.file) {
            order.push(hit.file.clone());
        }
        by_file.entry(hit.file.clone()).or_default().push(hit);
    }
    let cap = match opts.level {
        CompactLevel::Conservative => usize::MAX,
        CompactLevel::Balanced | CompactLevel::Aggressive => opts.per_file_cap.max(1),
    };
    let mut out = String::new();
    let mut emitted = 0usize;
    for file in order {
        let hits = by_file.get(&file).cloned().unwrap_or_default();
        let n = hits.len();
        if n == 0 {
            continue;
        }
        for h in hits.iter().take(cap) {
            if opts.total_cap > 0 && emitted >= opts.total_cap {
                break;
            }
            out.push_str(&format!("{}:{}: {}\n", h.file, h.line_no, h.line));
            emitted += 1;
        }
        if n > cap {
            out.push_str(&format!("    +{} more matches in {}\n", n - cap, file));
        }
        if opts.total_cap > 0 && emitted >= opts.total_cap {
            out.push_str(&format!(
                "... [{} more matches not shown — total cap reached]\n",
                matches.len().saturating_sub(emitted)
            ));
            break;
        }
    }
    out
}

pub fn compact_listing(entries: &[DirEntry], opts: &ListOpts) -> String {
    let mut hidden = 0u32;
    let mut directories: Vec<&DirEntry> = Vec::new();
    let mut others: Vec<&DirEntry> = Vec::new();
    for e in entries {
        if e.is_hidden {
            hidden += 1;
            continue;
        }
        if e.is_dir {
            directories.push(e);
        } else {
            others.push(e);
        }
    }
    let mut out = String::new();
    for d in &directories {
        out.push_str(&format!("{}/\n", d.name));
    }
    if matches!(opts.level, CompactLevel::Conservative) || !opts.group_by_ext {
        for f in &others {
            out.push_str(&f.name);
            out.push('\n');
        }
    } else {
        let mut groups: BTreeMap<String, u32> = BTreeMap::new();
        let mut bare: Vec<&str> = Vec::new();
        for f in &others {
            if let Some(dot) = f.name.rfind('.') {
                let ext = &f.name[dot + 1..];
                if !ext.is_empty() && ext.chars().all(|c| c.is_alphanumeric()) {
                    *groups.entry(ext.to_string()).or_default() += 1;
                    continue;
                }
            }
            bare.push(&f.name);
        }
        for n in bare {
            out.push_str(n);
            out.push('\n');
        }
        for (ext, n) in groups {
            out.push_str(&format!("*.{ext} ({n})\n"));
        }
    }
    if hidden > 0 {
        out.push_str(&format!("+{hidden} hidden\n"));
    }
    out
}
