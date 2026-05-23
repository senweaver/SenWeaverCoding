// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::chunker::Chunk;
use super::planner::QueryPlan;
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct Coverage {
    pub covered: Vec<String>,
    pub missing: Vec<String>,
}

pub fn coverage(chunks: &[Chunk], plan: &QueryPlan) -> Coverage {
    let mut seen: HashSet<String> = HashSet::new();
    for chunk in chunks {
        for token in &chunk.tokens_matched {
            seen.insert(token.clone());
        }
    }
    let mut covered: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for token in &plan.tokens {
        if seen.contains(token) {
            covered.push(token.clone());
        } else {
            missing.push(token.clone());
        }
    }
    Coverage { covered, missing }
}

pub fn format_report(coverage: &Coverage, reflection_added: usize) -> String {
    let mut out = String::new();
    out.push_str("## Reflection\n");
    if coverage.covered.is_empty() {
        out.push_str("- Covered tokens: (none)\n");
    } else {
        out.push_str(&format!("- Covered tokens: {}\n", coverage.covered.join(", ")));
    }
    if coverage.missing.is_empty() {
        out.push_str("- Missing tokens: (none)\n");
    } else {
        out.push_str(&format!("- Missing tokens: {}\n", coverage.missing.join(", ")));
    }
    if reflection_added > 0 {
        out.push_str(&format!("- Relaxed re-query added {reflection_added} chunk(s).\n"));
    } else if !coverage.missing.is_empty() {
        out.push_str("- Relaxed re-query did not yield new chunks.\n");
    }
    out
}
