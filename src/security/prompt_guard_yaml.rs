// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! YAML-driven rule set for [`super::prompt_guard::PromptGuard`].
//!
//! instead of hard-coding regex literals inside
//! [`super::prompt_guard`], this module exposes a declarative
//! [`PromptGuardRules`] structure that can be deserialised from
//! `prompt_guard.yaml`.  The existing guard falls back to its
//! built-in defaults when no YAML file is present, so this module is
//! strictly additive.
//!
//! ## File format
//!
//! ```yaml
//! categories:
//!   system_override:
//!     score: 1.0
//!     patterns:
//!       - "(?i)ignore\\s+(previous|all|above)\\s+instructions?"
//!   role_confusion:
//!     score: 0.9
//!     patterns:
//!       - "(?i)you\\s+are\\s+now\\s+"
//! ```
//!
//! Each regex is compiled once and cached on the resulting
//! [`CompiledRules`] value.  Callers typically load the file at
//! start-up and keep the compiled rules around for the lifetime of
//! the process.

use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptGuardRules {

    #[serde(default)]
    pub categories: BTreeMap<String, CategoryRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRules {

    pub score: f64,

    #[serde(default)]
    pub patterns: Vec<String>,
}

impl CategoryRules {

    pub fn clamped_score(&self) -> f64 {
        self.score.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone)]
pub struct CompiledRules {

    pub entries: Vec<(String, f64, Regex)>,

    pub compile_errors: Vec<CompileError>,
}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub category: String,
    pub pattern: String,
    pub message: String,
}

impl CompiledRules {

    pub fn from_rules(rules: &PromptGuardRules) -> Self {
        let mut entries = Vec::new();
        let mut compile_errors = Vec::new();
        for (cat, bundle) in &rules.categories {
            for pat in &bundle.patterns {
                match Regex::new(pat) {
                    Ok(r) => entries.push((cat.clone(), bundle.clamped_score(), r)),
                    Err(e) => compile_errors.push(CompileError {
                        category: cat.clone(),
                        pattern: pat.clone(),
                        message: e.to_string(),
                    }),
                }
            }
        }
        Self {
            entries,
            compile_errors,
        }
    }

    pub fn pattern_count(&self) -> usize {
        self.entries.len()
    }

    pub fn scan(&self, content: &str) -> (f64, Vec<String>) {
        let mut max = 0.0_f64;
        let mut hits: Vec<String> = Vec::new();
        for (cat, score, re) in &self.entries {
            if re.is_match(content) {
                max = max.max(*score);
                if !hits.iter().any(|h| h == cat) {
                    hits.push(cat.clone());
                }
            }
        }
        (max, hits)
    }
}

pub fn default_rules_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("SEN_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(
        std::path::PathBuf::from(home)
            .join(".sen")
            .join("prompt_guard.yaml"),
    )
}

pub fn load_from_file<P: AsRef<Path>>(path: P) -> std::io::Result<Option<CompiledRules>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let rules: PromptGuardRules = serde_yaml::from_str(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(CompiledRules::from_rules(&rules)))
}

pub fn load_default() -> Option<CompiledRules> {
    let path = default_rules_path()?;
    match load_from_file(&path) {
        Ok(Some(rules)) => Some(rules),
        _ => None,
    }
}
