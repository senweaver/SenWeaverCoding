// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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

        "typescript" | "ts" | "mts" | "cts" => {
            Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        }
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
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
