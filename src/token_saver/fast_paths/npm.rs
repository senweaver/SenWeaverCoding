// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::token_saver::pipeline;
use crate::token_saver::CompactContext;
use crate::token_saver::CompactLevel;
use once_cell::sync::Lazy;
use regex::Regex;

static INSTALL_NOISE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:npm warn |npm notice|npm WARN|npm http|npm verb|npm sill|⠋|⠙|⠹|⠸|⠼|⠴|⠦|⠧|⠇|⠏|Resolving:|Fetching:|Linking:|Building:|Lockfile|Progress|added \d+ package|removed \d+ package|changed \d+ package|up to date|found \d+ vulnerabilit)",
    )
    .expect("npm noise regex")
});

static TEST_FAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:✗|×|FAIL|FAILED|\u{2716}|\d+\) )\b").expect("npm test fail regex")
});

pub fn install(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    _ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr_scrub = pipeline::strip_ansi_only(raw_stderr);
    let stdout = drop_install_noise(&scrub);
    let stderr = drop_install_noise(&stderr_scrub);
    (stdout, stderr)
}

fn drop_install_noise(text: &str) -> String {
    let mut out = String::with_capacity(text.len() / 2);
    for line in text.lines() {
        if INSTALL_NOISE.is_match(line) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
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
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    let stdout = match ctx.level {
        CompactLevel::Conservative => drop_install_noise(&scrub),
        CompactLevel::Balanced | CompactLevel::Aggressive => keep_failures(&scrub),
    };
    (stdout, stderr)
}

fn keep_failures(text: &str) -> String {
    let mut out = String::with_capacity(text.len() / 4);
    let mut summary: Option<String> = None;
    for line in text.lines() {
        if TEST_FAIL_RE.is_match(line) {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("Tests:")
            || trimmed.starts_with("Test Files")
            || trimmed.starts_with("Snapshots:")
            || trimmed.starts_with("Suites:")
            || trimmed.starts_with("Time:")
        {
            summary = Some(match summary {
                Some(prev) => format!("{prev}\n{trimmed}"),
                None => trimmed.to_string(),
            });
        }
    }
    if let Some(s) = summary {
        if !out.is_empty() {
            out.push_str("---\n");
        }
        out.push_str(&s);
        out.push('\n');
    }
    if out.trim().is_empty() {
        text.to_string()
    } else {
        out
    }
}

pub fn run(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    _ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);
    let stdout = drop_install_noise(&scrub);
    (stdout, stderr)
}

pub fn lint_or_tsc(
    _cmd: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    _exit_code: i32,
    _ctx: &CompactContext,
) -> (String, String) {
    let scrub = pipeline::strip_ansi_only(raw_stdout);
    let stderr = pipeline::strip_ansi_only(raw_stderr);

    let stdout = drop_install_noise(&scrub);
    (stdout, stderr)
}
