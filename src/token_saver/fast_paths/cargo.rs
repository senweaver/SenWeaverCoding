// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::token_saver::pipeline;
use crate::token_saver::CompactContext;
use crate::token_saver::CompactLevel;
use once_cell::sync::Lazy;
use regex::Regex;

static PROGRESS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\s*(?:Compiling|Updating|Downloading|Downloaded|Checking|Documenting|Generating|Finished|Running|Fresh|Locking|Adding|Removing|Building|Installing|Uninstalling|Skipping|Building \[)\b",
    )
    .expect("cargo progress regex")
});

static WARN_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^warning(?:\[[^\]]+\])?:").expect("cargo warn header"));

static ERR_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^error(?:\[[^\]]+\])?:").expect("cargo err header"));

pub fn build_or_check(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {

    let drop_warnings = matches!(ctx.level, CompactLevel::Aggressive);
    let stdout = filter_diagnostics(raw_stdout, drop_warnings);
    let stderr = filter_diagnostics(raw_stderr, drop_warnings);
    (stdout, stderr)
}

fn filter_diagnostics(raw: &str, drop_warnings: bool) -> String {
    let scrub = pipeline::strip_ansi_only(raw);
    let mut out = String::with_capacity(scrub.len() / 2);
    let mut keeping_block = false;
    let mut block_kind: Option<bool> = None;
    for line in scrub.lines() {
        let is_progress = PROGRESS_RE.is_match(line);
        if is_progress {
            keeping_block = false;
            block_kind = None;
            continue;
        }
        let is_warn = WARN_HEADER_RE.is_match(line);
        let is_err = ERR_HEADER_RE.is_match(line);
        if is_warn || is_err {
            keeping_block = true;
            block_kind = Some(is_warn);
            if !(is_warn && drop_warnings) {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if line.trim().is_empty() {
            keeping_block = false;
            block_kind = None;
            out.push('\n');
            continue;
        }
        if keeping_block {
            let suppress = block_kind == Some(true) && drop_warnings;
            if !suppress {
                out.push_str(line);
                out.push('\n');
            }
        } else {

            out.push_str(line);
            out.push('\n');
        }
    }
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
}

pub fn test(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = filter_diagnostics(raw_stderr, matches!(ctx.level, CompactLevel::Aggressive));

    let mut summary_line: Option<String> = None;
    let mut failed: Vec<String> = Vec::new();
    let mut in_failures_block = false;
    let mut failure_buf = String::new();
    let mut current_failure: Option<String> = None;

    for line in scrub.lines() {
        if line.starts_with("test result:") {
            summary_line = Some(line.to_string());
        }
        if line.starts_with("failures:") {
            in_failures_block = true;
            continue;
        }
        if in_failures_block {
            if let Some(rest) = line.trim().strip_prefix("---- ") {
                if let Some(end) = rest.find(" stdout ----") {
                    let name = rest[..end].trim().to_string();
                    if let Some(prev) = current_failure.take() {
                        failed.push(format!("{prev}\n{}", failure_buf.trim_end()));
                        failure_buf.clear();
                    }
                    current_failure = Some(name);
                    continue;
                }
            }
            if current_failure.is_some() {
                failure_buf.push_str(line);
                failure_buf.push('\n');
            }
        }
    }
    if let Some(prev) = current_failure {
        failed.push(format!("{prev}\n{}", failure_buf.trim_end()));
    }

    let stdout = match ctx.level {
        CompactLevel::Conservative => {
            if failed.is_empty() && summary_line.is_some() {
                summary_line.clone().unwrap_or_default()
            } else {
                let mut out = String::new();
                if let Some(s) = &summary_line {
                    out.push_str(s);
                    out.push('\n');
                }
                if !failed.is_empty() {
                    out.push_str("---- failures ----\n");
                    for f in &failed {
                        out.push_str(f);
                        out.push_str("\n\n");
                    }
                }
                out
            }
        }
        CompactLevel::Balanced | CompactLevel::Aggressive => {
            let mut out = String::new();
            if let Some(s) = &summary_line {
                out.push_str(s);
                out.push('\n');
            }
            for f in &failed {
                let name = f.lines().next().unwrap_or("");
                if !name.is_empty() {
                    out.push_str(&format!("FAIL: {name}\n"));
                }
            }
            out
        }
    };
    (stdout, stderr)
}

pub fn generic(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    _ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    let mut out = String::with_capacity(scrub.len());
    for line in scrub.lines() {
        if PROGRESS_RE.is_match(line) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, stderr)
}
