// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Unified-diff renderer with optional scope annotation.
//!
//! [`UnifiedHunkRenderer`] is a lightweight formatting utility that post-
//! processes a unified diff string before it is forwarded to an LLM retry
//! prompt or stored in a [`super::llm_refine`] context.  When
//! `scope_annotation` is enabled, each `@@` hunk header is preceded by a
//! comment line that names the enclosing Rust/Python/… function at that line:
//!
//! ```text
//! // fn apply_batch @ L142
//! @@ -142,7 +142,9 @@
//! ```
//!
//! This annotation is intentionally stripped to a comment so that consumers
//! (heuristic applier, provider diff parsers) that do not understand it can
//! skip it without misinterpreting the hunk.

use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct UnifiedHunkRenderer {
    scope_annotation: bool,
}

impl UnifiedHunkRenderer {

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_scope_annotation(mut self, enabled: bool) -> Self {
        self.scope_annotation = enabled;
        self
    }

    pub fn render(&self, path: &Path, diff: &str) -> String {
        if !self.scope_annotation {
            return diff.to_string();
        }
        let mut output = String::with_capacity(diff.len() + 128);
        for line in diff.lines() {
            if line.starts_with("@@") {
                if let Some(old_start) = parse_hunk_old_start(line) {
                    if let Some(name) =
                        crate::code_intel::outline::locate_scope(path, old_start)
                    {
                        output.push_str(&format!("// fn {} @ L{}\n", name, old_start));
                    }
                }
            }
            output.push_str(line);
            output.push('\n');
        }

        while output.ends_with("\n\n") {
            output.pop();
        }
        output
    }
}

fn parse_hunk_old_start(header: &str) -> Option<u32> {
    let rest = header.strip_prefix("@@")?.trim_start();
    let after_minus = rest.strip_prefix('-')?;
    let digits: String = after_minus
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().ok()
}
