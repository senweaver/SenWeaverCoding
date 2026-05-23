// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub path: PathBuf,
    pub contents: String,
    pub language: Language,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {

    File,

    Patch,

    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Json,
    Toml,
    Markdown,

    Go,
    Java,
    C,
    Cpp,
    Unknown,
}

impl Language {
    pub fn from_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Self::Rust,
            Some("py") => Self::Python,
            Some("ts") | Some("tsx") => Self::TypeScript,
            Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Self::JavaScript,
            Some("json") => Self::Json,
            Some("toml") => Self::Toml,
            Some("md") | Some("markdown") => Self::Markdown,

            Some("go") => Self::Go,
            Some("java") => Self::Java,
            Some("c") | Some("h") => Self::C,
            Some("cpp") | Some("cxx") | Some("cc") | Some("hpp") | Some("hh") | Some("hxx") => {
                Self::Cpp
            }
            _ => Self::Unknown,
        }
    }

    pub fn grammar_id(self) -> Option<&'static str> {
        Some(match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Markdown => "markdown",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Unknown => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct VerificationIssue {
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub severity: IssueSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub verifier: &'static str,
    pub passed: bool,
    pub issues: Vec<VerificationIssue>,

    pub summary: String,
}

impl VerificationReport {
    pub fn ok(verifier: &'static str) -> Self {
        Self {
            verifier,
            passed: true,
            issues: Vec::new(),
            summary: String::new(),
        }
    }

    pub fn failed(verifier: &'static str, issues: Vec<VerificationIssue>, summary: String) -> Self {
        Self {
            verifier,
            passed: false,
            issues,
            summary,
        }
    }

    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| matches!(i.severity, IssueSeverity::Error))
            .count()
    }
}

#[async_trait]
pub trait Verifier: Send + Sync {

    fn name(&self) -> &'static str;

    async fn verify(&self, artifact: &Artifact) -> anyhow::Result<VerificationReport>;
}
