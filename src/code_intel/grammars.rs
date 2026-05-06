// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! centralised tree-sitter grammar registry.
//!
//! Before the language → grammar mapping was hard-coded in
//! [`crate::code_intel::outline::tree_sitter_outline`].  As the syntactic
//! verifier (`agent::verification::syntactic`) and the post-apply
//! validator (`apply_model::validator`) both grew tree-sitter back-ends,
//! we needed one source of truth so adding a new language is a single
//! edit instead of three.
//!
//! [`grammar_for`] resolves a stable language identifier (the same
//! strings used by [`crate::code_intel::outline::infer_language`] and
//! the LSP-style `Language` enum) to a concrete `tree_sitter::Language`.
//! The full mapping is feature-gated:
//!
//! - `code-intel` (umbrella) → rust / python / javascript / typescript /
//!   json / toml / markdown.  These are the always-on grammars.
//! - `code-intel-go` → go.
//! - `code-intel-java` → java.
//! - `code-intel-c` → c.
//! - `code-intel-cpp` → cpp / c++.
//!
//! Without `code-intel` the function compiles to a stub returning
//! `None` so the rest of the codebase continues to compile on minimal
//! builds.  Callers MUST treat `None` as "language unknown — degrade
//! to heuristic", never as a fatal error.

#![allow(dead_code)]

#[cfg(feature = "code-intel")]
use tree_sitter::Language;

#[cfg(feature = "code-intel")]
pub fn grammar_for(lang: &str) -> Option<Language> {
    match lang.to_ascii_lowercase().as_str() {
        "rust" | "rs" => Some(tree_sitter_rust::LANGUAGE.into()),
        "python" | "py" => Some(tree_sitter_python::LANGUAGE.into()),
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => {
            Some(tree_sitter_javascript::LANGUAGE.into())
        }

        "typescript" | "ts" | "tsx" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "json" => Some(tree_sitter_json::LANGUAGE.into()),
        "toml" => Some(tree_sitter_toml_ng::LANGUAGE.into()),
        "markdown" | "md" => Some(tree_sitter_md::LANGUAGE.into()),
        #[cfg(feature = "code-intel-go")]
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        #[cfg(feature = "code-intel-java")]
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        #[cfg(feature = "code-intel-c")]
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        #[cfg(feature = "code-intel-cpp")]
        "cpp" | "c++" | "cxx" | "cc" | "hpp" | "hh" | "hxx" => {
            Some(tree_sitter_cpp::LANGUAGE.into())
        }
        _ => None,
    }
}

#[cfg(not(feature = "code-intel"))]
pub fn grammar_for(_lang: &str) -> Option<()> {
    None
}
