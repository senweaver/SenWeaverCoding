// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! User instruction rules — loads markdown files from `~/.senweavercoding/rules/`
//! with a tiered loading strategy that preserves accuracy without wasting tokens.
//!
//! Tier resolution per file:
//!   * `alwaysApply: true` (or missing — the SAFE default) → full body is
//!     embedded into the system prompt every turn. Use this for hard
//!     constraints the assistant MUST always honour.
//!   * `alwaysApply: false` → only metadata (name, location, description /
//!     summary) is injected; the assistant calls `read_user_rule(name)` to
//!     pull the full content on demand. Use this for context-specific
//!     references (large knowledge bases, optional playbooks).
//!
//! The default is intentionally biased toward correctness: a user who drops
//! a markdown file into `~/.senweavercoding/rules/` without any frontmatter
//! gets the same eager behaviour as before, so existing setups never lose
//! constraint enforcement when this loader rolls out.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SUMMARY_CHAR_LIMIT: usize = 200;
const MAX_RULE_FILE_BYTES: u64 = 256 * 1024;
const EAGER_RULE_BODY_CHAR_LIMIT: usize = 20_000;

#[derive(Debug, Clone, Default)]
pub struct UserRuleFrontmatter {
    pub always_apply: Option<bool>,
    pub description: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserRuleMeta {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub summary: String,
    pub description: Option<String>,
    pub always_apply: bool,
    pub body: Option<String>,
    pub body_truncated: bool,
}

pub fn user_rules_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".senweavercoding").join("rules"))
}

pub fn list_user_rules() -> Vec<UserRuleMeta> {
    let Some(dir) = user_rules_dir() else {
        return Vec::new();
    };
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    walk_rule_files(&dir, 0, &mut paths);
    paths.sort();
    let mut out: Vec<UserRuleMeta> = Vec::with_capacity(paths.len());
    for path in paths {
        match build_rule_meta(&dir, &path) {
            Ok(meta) => out.push(meta),
            Err(err) => {
                tracing::debug!(
                    target: "user_rules",
                    path = %path.display(),
                    error = %err,
                    "skipped unreadable user rule file"
                );
            }
        }
    }
    out
}

fn build_rule_meta(root: &Path, path: &Path) -> io::Result<UserRuleMeta> {
    let metadata = fs::metadata(path)?;
    let size = metadata.len();
    let display_name = match path.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
    };

    let raw = if size <= MAX_RULE_FILE_BYTES {
        fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    let (frontmatter, body) = split_frontmatter(&raw);
    let parsed_fm = frontmatter
        .as_deref()
        .map(parse_simple_frontmatter)
        .unwrap_or_default();

    let body_trimmed = body.trim();
    let summary: String = body_trimmed.chars().take(SUMMARY_CHAR_LIMIT).collect();

    let display_name = parsed_fm.name.clone().unwrap_or(display_name);
    let always_apply = parsed_fm.always_apply.unwrap_or(true);

    let (eager_body, truncated) = if always_apply && !body_trimmed.is_empty() {
        let (clipped, was_truncated) = truncate_chars(body_trimmed, EAGER_RULE_BODY_CHAR_LIMIT);
        (Some(clipped.to_string()), was_truncated)
    } else {
        (None, false)
    };

    Ok(UserRuleMeta {
        name: display_name,
        path: path.to_path_buf(),
        size,
        summary,
        description: parsed_fm
            .description
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        always_apply,
        body: eager_body,
        body_truncated: truncated,
    })
}

pub fn read_user_rule(name: &str) -> Result<String, ReadRuleError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ReadRuleError::InvalidName);
    }
    let rules = list_user_rules();
    let matched = rules
        .iter()
        .find(|rule| rule.name.eq_ignore_ascii_case(trimmed))
        .or_else(|| {
            rules.iter().find(|rule| {
                rule.path
                    .file_name()
                    .map(|s| s.to_string_lossy().eq_ignore_ascii_case(trimmed))
                    .unwrap_or(false)
            })
        });
    let rule = matched.ok_or_else(|| ReadRuleError::NotFound {
        requested: trimmed.to_string(),
        available: rules.iter().map(|r| r.name.clone()).collect(),
    })?;
    if rule.size > MAX_RULE_FILE_BYTES {
        return Err(ReadRuleError::TooLarge {
            name: rule.name.clone(),
            size: rule.size,
            limit: MAX_RULE_FILE_BYTES,
        });
    }
    let raw = fs::read_to_string(&rule.path).map_err(|err| ReadRuleError::Io {
        path: rule.path.clone(),
        error: err,
    })?;
    let (_, body) = split_frontmatter(&raw);
    Ok(format_rule_payload(&rule.name, &rule.path, body.trim()))
}

fn format_rule_payload(name: &str, path: &Path, body: &str) -> String {
    let mut out = String::new();
    out.push_str("# User Instruction Rule: ");
    out.push_str(name);
    out.push('\n');
    out.push_str("Source: ");
    out.push_str(&path.display().to_string());
    out.push_str("\n\n");
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(Debug)]
pub enum ReadRuleError {
    InvalidName,
    NotFound {
        requested: String,
        available: Vec<String>,
    },
    TooLarge {
        name: String,
        size: u64,
        limit: u64,
    },
    Io {
        path: PathBuf,
        error: io::Error,
    },
}

impl std::fmt::Display for ReadRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadRuleError::InvalidName => write!(f, "rule name must be a non-empty string"),
            ReadRuleError::NotFound {
                requested,
                available,
            } => {
                let listed = if available.is_empty() {
                    "none".to_string()
                } else {
                    available.join(", ")
                };
                write!(
                    f,
                    "user rule '{requested}' not found. Available rules: {listed}"
                )
            }
            ReadRuleError::TooLarge { name, size, limit } => {
                write!(
                    f,
                    "user rule '{name}' is too large ({size} bytes > {limit} byte limit)"
                )
            }
            ReadRuleError::Io { path, error } => {
                write!(f, "failed to read {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for ReadRuleError {}

fn walk_rule_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 1 {
                walk_rule_files(&path, depth + 1, out);
            }
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let lc = ext.to_ascii_lowercase();
        if lc == "md" || lc == "mdc" {
            out.push(path);
        }
    }
}

fn truncate_chars(content: &str, max_chars: usize) -> (&str, bool) {
    if content.chars().count() <= max_chars {
        return (content, false);
    }
    let cut = content
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(content.len());
    (&content[..cut], true)
}

fn home_dir() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    None
}

fn split_frontmatter(content: &str) -> (Option<String>, String) {
    let normalized = content.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return (None, normalized);
    };
    if let Some(idx) = rest.find("\n---\n") {
        let frontmatter = rest[..idx].to_string();
        let body = rest[idx + 5..].to_string();
        return (Some(frontmatter), body);
    }
    if let Some(frontmatter) = rest.strip_suffix("\n---") {
        return (Some(frontmatter.to_string()), String::new());
    }
    (None, normalized)
}

fn parse_simple_frontmatter(s: &str) -> UserRuleFrontmatter {
    let mut meta = UserRuleFrontmatter::default();
    for line in s.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let key_norm = key.trim().to_ascii_lowercase().replace('-', "_");
        let val = val.trim().trim_matches('"').trim_matches('\'');
        match key_norm.as_str() {
            "always_apply" | "alwaysapply" => {
                meta.always_apply = parse_bool(val);
            }
            "description" => {
                if !val.is_empty() {
                    meta.description = Some(val.to_string());
                }
            }
            "name" => {
                if !val.is_empty() {
                    meta.name = Some(val.to_string());
                }
            }
            _ => {}
        }
    }
    meta
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => Some(true),
        "false" | "no" | "0" | "off" => Some(false),
        _ => None,
    }
}

pub fn user_rules_to_prompt(rules: &[UserRuleMeta]) -> String {
    use std::fmt::Write as _;

    if rules.is_empty() {
        return String::new();
    }

    let always: Vec<&UserRuleMeta> = rules.iter().filter(|r| r.always_apply).collect();
    let lazy: Vec<&UserRuleMeta> = rules.iter().filter(|r| !r.always_apply).collect();

    let mut out = String::from("## User Instruction Rules\n\n");
    out.push_str(
        "These rules express user-level constraints and conventions. Follow the always-applied \
         rules without exception. For lazy rules, scan their `<name>` and `<description>` \
         and call `read_user_rule(name)` to load the full body the moment any of them might \
         apply to the current task.\n\n",
    );

    if !always.is_empty() {
        out.push_str("### Always-Applied Rules\n\n");
        out.push_str(
            "The following rules are loaded eagerly because they declare \
             `alwaysApply: true` or have no frontmatter. Treat their contents as \
             binding constraints on every response.\n\n",
        );
        for rule in &always {
            let _ = writeln!(out, "#### {}", rule.name);
            let _ = writeln!(out, "_Source: {}_\n", rule.path.display());
            if let Some(body) = rule.body.as_deref() {
                out.push_str(body.trim());
                out.push_str("\n\n");
                if rule.body_truncated {
                    let _ = writeln!(
                        out,
                        "_[truncated at {EAGER_RULE_BODY_CHAR_LIMIT} chars — call \
                         `read_user_rule(name=\"{}\")` for the full body]_\n",
                        rule.name
                    );
                }
            } else if !rule.summary.is_empty() {
                out.push_str(&rule.summary);
                out.push_str("\n\n");
            }
        }
    }

    if !lazy.is_empty() {
        out.push_str("### Available Rules (loaded on demand)\n\n");
        out.push_str(
            "Only metadata is preloaded for these rules to keep context compact. \
             They are tagged `alwaysApply: false`. When the current task touches the \
             topic of any rule below, call `read_user_rule(name)` to load its full body \
             before acting.\n\n",
        );
        out.push_str("<available_user_rules>\n");
        for rule in &lazy {
            let _ = writeln!(out, "  <rule>");
            write_xml_text_element(&mut out, 4, "name", &rule.name);
            write_xml_text_element(&mut out, 4, "location", &rule.path.display().to_string());
            let _ = writeln!(out, "    <size>{} bytes</size>", rule.size);
            if let Some(desc) = rule.description.as_deref() {
                write_xml_text_element(&mut out, 4, "description", desc);
            }
            if !rule.summary.is_empty() {
                write_xml_text_element(&mut out, 4, "summary", &rule.summary);
            }
            let _ = writeln!(out, "  </rule>");
        }
        out.push_str("</available_user_rules>\n");
    }

    out.trim_end().to_string()
}

fn write_xml_text_element(buf: &mut String, indent: usize, tag: &str, value: &str) {
    use std::fmt::Write as _;
    let pad = " ".repeat(indent);
    if value.contains('\n') || value.len() > 80 {
        let _ = writeln!(buf, "{pad}<{tag}><![CDATA[{value}]]></{tag}>");
    } else {
        let escaped = value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let _ = writeln!(buf, "{pad}<{tag}>{escaped}</{tag}>");
    }
}
