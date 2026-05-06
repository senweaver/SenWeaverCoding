// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tier-1 language-server pool manifest.
//!
//! SenWeaverCoding supports many language servers (see
//! [`super::lsp::server_candidates`]), but five of them are the
//! product's "tier-1" surface: they are documented, tested, exposed
//! through `tools::lsp_symbols`, and surfaced in the GUI status
//! widget.  This module centralises that list so callers (docs, GUI
//! badges, readiness checks) all reference the same source of truth.
//!
//! The pool is intentionally small; new languages should prove
//! themselves through the generic LSP plumbing first, then get
//! promoted here with a dedicated test matrix.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServerSpec {

    pub language: &'static str,

    pub display_name: &'static str,

    pub preferred_binaries: &'static [&'static str],
}

impl LspServerSpec {

    pub fn detect(&self) -> Option<PathBuf> {
        for name in self.preferred_binaries {
            if let Ok(p) = which::which(name) {
                return Some(p);
            }
        }
        None
    }

    pub fn is_available(&self) -> bool {
        self.detect().is_some()
    }
}

pub const TIER1_POOL: &[LspServerSpec] = &[
    LspServerSpec {
        language: "rust",
        display_name: "Rust",
        preferred_binaries: &["rust-analyzer"],
    },
    LspServerSpec {
        language: "python",
        display_name: "Python",
        preferred_binaries: &["pyright-langserver", "pyright", "pylsp"],
    },
    LspServerSpec {
        language: "typescript",
        display_name: "TypeScript / JavaScript",
        preferred_binaries: &["typescript-language-server", "tsserver"],
    },
    LspServerSpec {
        language: "go",
        display_name: "Go",
        preferred_binaries: &["gopls"],
    },
    LspServerSpec {
        language: "cpp",
        display_name: "C / C++",
        preferred_binaries: &["clangd"],
    },
];

#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub language: &'static str,
    pub display_name: &'static str,
    pub available: bool,
    pub binary_path: Option<PathBuf>,
}

pub fn pool_status() -> Vec<PoolStatus> {
    TIER1_POOL
        .iter()
        .map(|spec| {
            let binary_path = spec.detect();
            PoolStatus {
                language: spec.language,
                display_name: spec.display_name,
                available: binary_path.is_some(),
                binary_path,
            }
        })
        .collect()
}

pub fn find(language: &str) -> Option<&'static LspServerSpec> {
    TIER1_POOL.iter().find(|s| s.language == language)
}
