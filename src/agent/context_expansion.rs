// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use crate::context_resolver::{
    ContextBudget, ContextItem, DefaultResolver, parse_context_tags, strip_context_tags,
};

pub const DEFAULT_BUDGET_TOKENS: usize = 8_192;

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
        return input.to_string();
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
        return prose;
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
