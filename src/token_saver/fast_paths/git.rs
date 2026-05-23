// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::token_saver::pipeline;
use crate::token_saver::CompactContext;
use crate::token_saver::CompactLevel;

pub fn status(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    if exit_code != 0 {
        return super::ansi_only(raw_stdout, raw_stderr);
    }
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let mut staged = 0u32;
    let mut unstaged = 0u32;
    let mut untracked = 0u32;
    let mut branch: Option<String> = None;
    for line in scrub.lines() {
        let t = line.trim_start();
        if let Some(b) = t.strip_prefix("On branch ") {
            branch = Some(b.trim().to_string());
        } else if t.starts_with("Changes to be committed:") {

        } else if t.starts_with("Changes not staged") {

        } else if t.starts_with("Untracked files:") {

        }

        if line.starts_with('\t') {

            let entry = line.trim_start();
            if entry.starts_with("modified:")
                || entry.starts_with("new file:")
                || entry.starts_with("renamed:")
                || entry.starts_with("deleted:")
                || entry.starts_with("typechange:")
            {

                unstaged = unstaged.saturating_add(1);
            } else if !entry.is_empty() {
                untracked = untracked.saturating_add(1);
            }
        }

        if line.len() >= 3 && line.as_bytes()[2] == b' ' {
            let xy = &line[..2];
            let x = xy.as_bytes()[0];
            let y = xy.as_bytes()[1];
            if x != b' ' && x != b'?' {
                staged = staged.saturating_add(1);
            }
            if y != b' ' && y != b'?' {
                unstaged = unstaged.saturating_add(1);
            }
            if x == b'?' && y == b'?' {
                untracked = untracked.saturating_add(1);
            }
        }
    }
    let branch = branch.unwrap_or_else(|| "HEAD".to_string());
    let summary = format!(
        "branch: {branch}  staged: {staged}  unstaged: {unstaged}  untracked: {untracked}"
    );
    let stdout = match ctx.level {
        CompactLevel::Conservative => {

            format!("{summary}\n---\n{}", scrub.trim_end())
        }
        CompactLevel::Balanced | CompactLevel::Aggressive => summary,
    };
    (stdout, pipeline::strip_ansi_only(raw_stderr))
}

pub fn log(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    if exit_code != 0 {
        return super::ansi_only(raw_stdout, raw_stderr);
    }
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stdout = match ctx.level {
        CompactLevel::Conservative => scrub,
        CompactLevel::Balanced | CompactLevel::Aggressive => {
            let mut current_hash: Option<String> = None;
            let mut current_msg: Option<String> = None;
            let mut out = String::new();
            for line in scrub.lines() {
                if let Some(rest) = line.strip_prefix("commit ") {
                    if let (Some(h), Some(m)) = (current_hash.take(), current_msg.take()) {
                        out.push_str(&format!("{} {}\n", &h[..h.len().min(7)], m));
                    }
                    current_hash = Some(rest.split_whitespace().next().unwrap_or(rest).to_string());
                } else if current_msg.is_none() && !line.starts_with("Author:") && !line.starts_with("Date:") {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        current_msg = Some(trimmed.to_string());
                    }
                }
            }
            if let (Some(h), Some(m)) = (current_hash, current_msg) {
                out.push_str(&format!("{} {}\n", &h[..h.len().min(7)], m));
            }
            if out.is_empty() { scrub } else { out }
        }
    };
    (stdout, pipeline::strip_ansi_only(raw_stderr))
}

pub fn diff(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    if exit_code != 0 {
        return super::ansi_only(raw_stdout, raw_stderr);
    }
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stdout = match ctx.level {
        CompactLevel::Conservative => scrub,
        CompactLevel::Balanced | CompactLevel::Aggressive => summarise_diff(&scrub),
    };
    (stdout, pipeline::strip_ansi_only(raw_stderr))
}

fn summarise_diff(diff: &str) -> String {
    let mut out = String::with_capacity(diff.len() / 4);
    let mut adds = 0u64;
    let mut dels = 0u64;
    for line in diff.lines() {
        if line.starts_with("diff --git")
            || line.starts_with("+++ ")
            || line.starts_with("--- ")
            || line.starts_with("@@")
            || line.starts_with("index ")
            || line.starts_with("similarity index")
            || line.starts_with("rename ")
            || line.starts_with("new file")
            || line.starts_with("deleted file")
        {
            if adds + dels > 0 {
                out.push_str(&format!("    [+{adds} -{dels}]\n"));
                adds = 0;
                dels = 0;
            }
            out.push_str(line);
            out.push('\n');
        } else if line.starts_with('+') && !line.starts_with("+++") {
            adds += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            dels += 1;
        }
    }
    if adds + dels > 0 {
        out.push_str(&format!("    [+{adds} -{dels}]\n"));
    }
    out
}

pub fn add(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    short_ack(raw_stdout, raw_stderr, exit_code, ctx, "ok")
}

pub fn commit(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    if exit_code != 0 || matches!(ctx.level, CompactLevel::Conservative) {
        return super::ansi_only(raw_stdout, raw_stderr);
    }
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let mut sha = String::new();
    for line in scrub.lines() {

        if let Some(start) = line.find('[') {
            if let Some(end) = line[start..].find(']') {
                let inside = &line[start + 1..start + end];
                if let Some((_, rhs)) = inside.split_once(' ') {
                    sha = rhs.trim().to_string();
                }
            }
        }
    }
    let stdout = if sha.is_empty() {
        "ok".to_string()
    } else {
        format!("ok {}", &sha[..sha.len().min(7)])
    };
    (stdout, pipeline::strip_ansi_only(raw_stderr))
}

pub fn push(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    if exit_code != 0 || matches!(ctx.level, CompactLevel::Conservative) {
        return super::ansi_only(raw_stdout, raw_stderr);
    }
    let scrub = pipeline::strip_ansi_only(raw_stderr);
    let mut branch = String::new();
    for line in scrub.lines() {
        let trimmed = line.trim();
        if let Some(arrow_at) = trimmed.find("->") {
            let rhs = trimmed[arrow_at + 2..].trim();
            if !rhs.is_empty() {
                branch = rhs.split_whitespace().next().unwrap_or("").to_string();
                break;
            }
        }
    }
    let stdout = if branch.is_empty() {
        "ok".to_string()
    } else {
        format!("ok {branch}")
    };
    (stdout, String::new())
}

pub fn pull(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    if exit_code != 0 || matches!(ctx.level, CompactLevel::Conservative) {
        return super::ansi_only(raw_stdout, raw_stderr);
    }
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let mut tail = String::new();
    for line in scrub.lines() {
        let trimmed = line.trim();
        if trimmed.contains("file changed") || trimmed.contains("files changed") {
            tail = trimmed.to_string();
            break;
        }
    }
    let stdout = if tail.is_empty() {
        "ok".to_string()
    } else {
        format!("ok ({tail})")
    };
    (stdout, pipeline::strip_ansi_only(raw_stderr))
}

pub fn generic_short_ack(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    short_ack(raw_stdout, raw_stderr, exit_code, ctx, "ok")
}

fn short_ack(
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
    base: &str,
) -> (String, String) {
    if exit_code != 0 || matches!(ctx.level, CompactLevel::Conservative) {
        return super::ansi_only(raw_stdout, raw_stderr);
    }
    let scrub_out = pipeline::strip_ansi_only(raw_stdout);
    let scrub_err = pipeline::strip_ansi_only(raw_stderr);
    let trimmed_out = scrub_out.trim();
    if trimmed_out.is_empty() {
        return (base.to_string(), scrub_err);
    }

    let last = trimmed_out
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or("");
    if !last.is_empty() && last.len() < 120 {
        (format!("{base} {last}"), scrub_err)
    } else {
        (base.to_string(), scrub_err)
    }
}
