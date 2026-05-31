// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::token_saver::CompactLevel;

#[derive(Debug, Clone, Default)]
pub struct Rule {
    pub name: String,
    pub strip_lines: Vec<Regex>,
    pub replace: Vec<(Regex, String)>,
    pub max_lines: Option<usize>,
    pub head: Option<usize>,
    pub tail: Option<usize>,
    pub dedup: bool,
    pub on_empty: Option<String>,

    pub match_command: Option<Regex>,
}

impl Rule {

    pub fn compile(
        name: impl Into<String>,
        strip_lines: Vec<String>,
        replace: Vec<(String, String)>,
        max_lines: Option<usize>,
        head: Option<usize>,
        tail: Option<usize>,
        dedup: bool,
        on_empty: Option<String>,
        match_command: Option<String>,
    ) -> Self {
        let name = name.into();
        let strip_lines = strip_lines
            .into_iter()
            .filter_map(|p| match Regex::new(&p) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!(rule = %name, pattern = %p, error = %e, "invalid strip_lines regex; skipping");
                    None
                }
            })
            .collect();
        let replace = replace
            .into_iter()
            .filter_map(|(pat, rep)| match Regex::new(&pat) {
                Ok(r) => Some((r, rep)),
                Err(e) => {
                    tracing::warn!(rule = %name, pattern = %pat, error = %e, "invalid replace regex; skipping");
                    None
                }
            })
            .collect();
        let match_command = match_command.and_then(|p| match Regex::new(&p) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!(rule = %name, pattern = %p, error = %e, "invalid match_command regex; skipping");
                None
            }
        });
        Self {
            name,
            strip_lines,
            replace,
            max_lines,
            head,
            tail,
            dedup,
            on_empty,
            match_command,
        }
    }

    pub fn matches_command(&self, command: &str) -> bool {
        match &self.match_command {
            Some(r) => r.is_match(command),
            None => true,
        }
    }
}

static ANSI_RE: Lazy<Regex> = Lazy::new(|| {

    Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]")
        .expect("static ANSI regex compiles")
});

pub fn apply(rule: &Rule, raw: &str, level: CompactLevel) -> String {
    let ansi_stripped = strip_ansi(raw);
    let replaced = replace_all(rule, &ansi_stripped);
    let line_stripped = strip_lines(rule, &replaced);
    let deduped = if rule.dedup {
        dedup_consecutive(&line_stripped)
    } else {
        line_stripped
    };
    let capped = head_tail_cap(&deduped, rule, level);
    if capped.trim().is_empty() {
        if let Some(empty) = &rule.on_empty {
            return empty.clone();
        }
    }
    capped
}

pub fn strip_ansi_only(raw: &str) -> String {
    strip_ansi(raw)
}

fn strip_ansi(raw: &str) -> String {
    ANSI_RE.replace_all(raw, "").into_owned()
}

fn replace_all(rule: &Rule, text: &str) -> String {
    let mut buf = text.to_string();
    for (pat, rep) in &rule.replace {
        buf = pat.replace_all(&buf, rep.as_str()).into_owned();
    }
    buf
}

fn strip_lines(rule: &Rule, text: &str) -> String {
    if rule.strip_lines.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let strip = rule.strip_lines.iter().any(|r| r.is_match(line));
        if !strip {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

pub fn dedup_consecutive(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<&str> = None;
    let mut count: u64 = 0;
    for line in text.lines() {
        match prev {
            Some(p) if p == line => count += 1,
            Some(p) => {
                flush_dedup(&mut out, p, count);
                prev = Some(line);
                count = 1;
            }
            None => {
                prev = Some(line);
                count = 1;
            }
        }
    }
    if let Some(p) = prev {
        flush_dedup(&mut out, p, count);
    }
    out
}

fn flush_dedup(out: &mut String, line: &str, count: u64) {
    if count >= 2 {
        out.push_str(line);
        out.push_str(&format!(" (x{count})"));
        out.push('\n');
    } else {
        out.push_str(line);
        out.push('\n');
    }
}

fn head_tail_cap(text: &str, rule: &Rule, level: CompactLevel) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();

    let (head, tail) = match level {
        CompactLevel::Conservative => (None, None),
        CompactLevel::Balanced | CompactLevel::Aggressive => (rule.head, rule.tail),
    };
    let max_lines = rule.max_lines;

    let cap_total = match (head, tail, max_lines) {
        (Some(h), Some(t), _) if h + t < total => Some(h + t),
        (Some(h), None, _) if h < total => Some(h),
        (None, Some(t), _) if t < total => Some(t),
        (None, None, Some(m)) if m < total => Some(m),
        _ => None,
    };

    let Some(_keep) = cap_total else {
        return text.to_string();
    };

    let mut out = String::with_capacity(text.len());
    if let (Some(h), Some(t)) = (head, tail) {
        for l in lines.iter().take(h) {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str(&format!("... [{} lines elided] ...\n", total - h - t));
        for l in lines.iter().skip(total - t) {
            out.push_str(l);
            out.push('\n');
        }
    } else if let Some(h) = head {
        for l in lines.iter().take(h) {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str(&format!("... [{} more lines]\n", total - h));
    } else if let Some(t) = tail {
        out.push_str(&format!("... [{} earlier lines]\n", total - t));
        for l in lines.iter().skip(total - t) {
            out.push_str(l);
            out.push('\n');
        }
    } else if let Some(m) = max_lines {
        for l in lines.iter().take(m) {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str(&format!("... [{} more lines truncated]\n", total - m));
    }
    out
}
