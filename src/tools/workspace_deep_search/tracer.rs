// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::chunker::Chunk;
use super::planner::QueryPlan;

pub fn render(
    query: &str,
    plan: &QueryPlan,
    chunks: &[Chunk],
    reflection: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Workspace DeepSearch: {query}\n\n"));
    out.push_str(&format!(
        "Intent: {:?} | Tokens: {} | Phrases: {}\n",
        plan.intent,
        plan.tokens.join(", "),
        if plan.phrases.is_empty() {
            "(none)".to_string()
        } else {
            plan.phrases.join(" | ")
        }
    ));
    if chunks.is_empty() {
        out.push_str("\nNo workspace chunks matched the query plan.\n");
        if let Some(report) = reflection {
            out.push('\n');
            out.push_str(report);
        }
        return out;
    }
    out.push_str(&format!("\nReturning top {} chunks (paragraph-level, traceable):\n\n", chunks.len()));
    for (idx, chunk) in chunks.iter().enumerate() {
        let rel = chunk.rel_path.to_string_lossy();
        let cite = format!("{}:{}-{}", rel, chunk.line_start, chunk.line_end);
        out.push_str(&format!(
            "## {}. {cite}  (score={:.3})\n",
            idx + 1,
            chunk.raw_score
        ));
        if !chunk.tokens_matched.is_empty() {
            out.push_str(&format!(
                "Tokens: {}\n",
                chunk.tokens_matched.join(", ")
            ));
        }
        let trimmed = truncate_chunk_body(&chunk.body, 1600);
        out.push_str("```text\n");
        out.push_str(&trimmed);
        if !trimmed.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
        out.push_str(&format!("Cite: `{cite}`\n\n"));
    }
    if let Some(report) = reflection {
        out.push_str(report);
    }
    out
}

fn truncate_chunk_body(body: &str, max_chars: usize) -> String {
    let count = body.chars().count();
    if count <= max_chars {
        return body.to_string();
    }
    let mut s: String = body.chars().take(max_chars).collect();
    s.push_str("\n… (truncated)");
    s
}
