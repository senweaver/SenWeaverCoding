// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use once_cell::sync::Lazy;
use regex::Regex;

const SMART_HEAD_LINES: usize = 80;
const SMART_TAIL_LINES: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadLevel {
    Default,
    Smart,
    Signatures,
}

impl ReadLevel {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "smart" => Self::Smart,
            "signatures" | "signature" | "sig" => Self::Signatures,
            _ => Self::Default,
        }
    }
}

pub fn compact(path: &str, content: &str, level: ReadLevel) -> String {
    match level {
        ReadLevel::Default => content.to_string(),
        ReadLevel::Smart => smart_compact(content),
        ReadLevel::Signatures => signatures_compact(path, content),
    }
}

fn smart_compact(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if total <= SMART_HEAD_LINES + SMART_TAIL_LINES {
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len() / 4);
    for l in lines.iter().take(SMART_HEAD_LINES) {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(&format!(
        "... [{} lines elided — use level=default for full content] ...\n",
        total - SMART_HEAD_LINES - SMART_TAIL_LINES
    ));
    for l in lines.iter().skip(total - SMART_TAIL_LINES) {
        out.push_str(l);
        out.push('\n');
    }
    out
}

static RUST_SIG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\s*(?:#\[[^\]]*\]\s*)?(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:const\s+)?(?:fn|struct|enum|trait|impl|type|const|static|mod)\b",
    )
    .expect("rust signature regex")
});

static PY_SIG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:async\s+)?(?:def|class)\s+\w+|^\s*@\w+").expect("python signature regex")
});

static JS_SIG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\s+\w+|class\s+\w+|interface\s+\w+|type\s+\w+|enum\s+\w+|const\s+\w+\s*=\s*(?:\([^)]*\)|async\s*\(|function))",
    )
    .expect("js/ts signature regex")
});

static GO_SIG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:func|type|var|const|package|import)\b").expect("go signature regex")
});

static C_SIG: Lazy<Regex> = Lazy::new(|| {

    Regex::new(
        r"^\s*(?:#include|#define|typedef\b|struct\s+\w+|enum\s+\w+|union\s+\w+|extern\b|static\b\s+\w+|\w[\w\s\*]*\s+\w+\s*\([^)]*\)\s*\{?)",
    )
    .expect("c/cpp signature regex")
});

static JAVA_SIG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\s*(?:package|import|(?:public|private|protected|static|final|abstract|synchronized)\s+).*",
    )
    .expect("java signature regex")
});

fn signatures_compact(path: &str, content: &str) -> String {
    let ext = path
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let re = match ext.as_str() {
        "rs" => &*RUST_SIG,
        "py" => &*PY_SIG,
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => &*JS_SIG,
        "go" => &*GO_SIG,
        "c" | "h" | "cc" | "cpp" | "hpp" | "hh" | "cxx" => &*C_SIG,
        "java" | "kt" | "scala" => &*JAVA_SIG,
        _ => return smart_compact(content),
    };
    let mut out = String::with_capacity(content.len() / 4);
    let mut hits = 0u32;
    for line in content.lines() {
        if re.is_match(line) {
            out.push_str(line);
            out.push('\n');
            hits += 1;
        }
    }
    if hits == 0 {

        return smart_compact(content);
    }
    out.push_str(&format!(
        "\n[signatures only — {hits} declarations; use level=default for full content]\n"
    ));
    out
}
