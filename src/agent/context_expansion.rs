// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Unified `@` reference expansion driven by the
//! [`crate::context_resolver`] stack.
//!
//! Before M1.3 each surface (CLI / TUI / GUI) called the legacy
//! [`crate::agent::loop_::expand_at_file_references`] helper which
//! only understood `@path/to/file` references and produced
//! ad-hoc `<file>` blocks.  The legacy expander is kept around for
//! backwards compatibility but every call site now goes through
//! [`expand_input`], which:
//!
//! 1. parses every recognised `@tag` token via
//!    [`crate::context_resolver::parse_context_tags`],
//! 2. resolves them through a [`crate::context_resolver::DefaultResolver`]
//!    so the same `@file:`, `@folder:`, `@symbol:`, `@diff:`,
//!    `@codebase:` etc. handlers run on every surface,
//! 3. falls back to the legacy expander when no resolver tag was
//!    detected — this keeps simple `@some-file.ext` references
//!    working while we transition older call sites.
//!
//! All resolver handlers are synchronous, so this expander is
//! synchronous as well.  Async surfaces (the agent loop, the user
//! turn pipeline) call it directly without `block_on` overhead.

use std::path::{Path, PathBuf};

use crate::context_resolver::{
    ContextBudget, ContextItem, DefaultResolver, parse_context_tags, strip_context_tags,
};

pub const DEFAULT_BUDGET_TOKENS: usize = 8_192;

const LEGACY_FALLBACK_BUDGET_BYTES: usize = 200_000;

pub fn expand_input(
    input: &str,
    workspace: &Path,
    recent_files: Vec<PathBuf>,
    current_selection: String,
) -> String {
    expand_input_with_budget(
        input,
        workspace,
        recent_files,
        current_selection,
        DEFAULT_BUDGET_TOKENS,
    )
}

pub fn expand_input_with_budget(
    input: &str,
    workspace: &Path,
    recent_files: Vec<PathBuf>,
    current_selection: String,
    budget_tokens: usize,
) -> String {
    let tags = parse_context_tags(input);
    if tags.is_empty() {
        return legacy_fallback(input, workspace);
    }

    let resolver = DefaultResolver::new(workspace.to_path_buf())
        .with_recent(recent_files)
        .with_selection(current_selection);
    let budget = ContextBudget::new(budget_tokens.max(1));

    let mut items: Vec<ContextItem> = Vec::with_capacity(tags.len());
    for tag in tags {
        match crate::context_resolver::handlers::resolve_tag(
            &tag,
            workspace,
            &resolver.recent_files,
            &resolver.current_selection,
            &budget,
        ) {
            Ok(item) => items.push(item),
            Err(err) => {
                tracing::debug!(
                    target: "context_expansion",
                    tag = %tag.label(),
                    error = %err,
                    "skipping unresolvable @tag"
                );
            }
        }
    }

    let prose = strip_context_tags(input);
    if items.is_empty() {
        return legacy_fallback(&prose, workspace);
    }

    let mut out = String::with_capacity(prose.len() + items.iter().map(|i| i.body.len()).sum::<usize>());
    out.push_str(prose.trim_end());
    for item in items {
        out.push_str("\n\n");
        out.push_str(&format!(
            "<context tag=\"{tag}\" title=\"{title}\" source=\"{source}\">\n{body}\n</context>",
            tag = item.tag,
            title = item.title,
            source = item.source,
            body = item.body,
        ));
    }
    out
}

fn legacy_fallback(input: &str, workspace: &Path) -> String {
    if !input.contains('@') {
        return input.to_string();
    }
    #[allow(deprecated)]
    let expanded = crate::agent::loop_::expand_at_file_references(input, workspace);
    if expanded.len() > LEGACY_FALLBACK_BUDGET_BYTES {
        let mut end = LEGACY_FALLBACK_BUDGET_BYTES;
        while end > 0 && !expanded.is_char_boundary(end) {
            end -= 1;
        }
        let mut clipped = String::with_capacity(end + 32);
        clipped.push_str(&expanded[..end]);
        clipped.push_str("\n\n[... truncated by context_expansion fallback ...]");
        return clipped;
    }
    expanded
}
