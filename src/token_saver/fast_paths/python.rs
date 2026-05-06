// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::token_saver::pipeline;
use crate::token_saver::CompactContext;
use crate::token_saver::CompactLevel;
use once_cell::sync::Lazy;
use regex::Regex;

static PYTEST_FAILURE_HEADER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^=+\s*FAILURES\s*=+\s*$").expect("pytest fail header"));
static PYTEST_SUMMARY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^=+\s*(?:short test summary info|warnings summary|.*?passed|.*?failed|.*?error).*=+\s*$").expect("pytest summary"));

pub fn pytest(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    let stdout = match ctx.level {
        CompactLevel::Conservative => drop_pytest_progress(&scrub),
        CompactLevel::Balanced | CompactLevel::Aggressive => failures_only(&scrub),
    };
    (stdout, stderr)
}

fn drop_pytest_progress(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {

        let trimmed = line.trim();
        if trimmed.contains("..F")
            || trimmed.contains("..E")
            || trimmed.starts_with("collecting ...")
            || trimmed.starts_with("collected ")
        {

            if trimmed.starts_with("collected ") {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn failures_only(text: &str) -> String {
    let mut out = String::new();
    let mut in_failures = false;
    let mut summary: Option<String> = None;
    for line in text.lines() {
        if PYTEST_FAILURE_HEADER.is_match(line) {
            in_failures = true;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_failures {
            if line.starts_with("====") && !PYTEST_FAILURE_HEADER.is_match(line) {
                in_failures = false;
            } else {
                out.push_str(line);
                out.push('\n');
                continue;
            }
        }
        if PYTEST_SUMMARY.is_match(line) {
            summary = Some(line.to_string());
        }
    }
    if let Some(s) = summary {
        if !out.is_empty() {
            out.push_str("---\n");
        }
        out.push_str(&s);
        out.push('\n');
    }
    if out.trim().is_empty() { text.to_string() } else { out }
}

pub fn ruff(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    _ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    (scrub, stderr)
}

pub fn pip(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    _ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    let mut out = String::with_capacity(scrub.len() / 2);
    for line in scrub.lines() {
        let t = line.trim_start();
        if t.starts_with("Looking in indexes")
            || t.starts_with("Requirement already satisfied")
            || t.starts_with("Using cached")
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, stderr)
}
