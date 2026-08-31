// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use crate::context_resolver::{
    ContextBudget, ContextItem, DefaultResolver, parse_context_tags_with_spans, strip_spans,
};

pub const DEFAULT_BUDGET_TOKENS: usize = 8_192;

pub async fn expand_input(
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
    .await
}

pub async fn expand_input_with_budget(
    input: &str,
    workspace: &Path,
    recent_files: Vec<PathBuf>,
    current_selection: String,
    budget_tokens: usize,
) -> String {
    let tagged = parse_context_tags_with_spans(input);
    if tagged.is_empty() {
        return input.to_string();
    }

    let resolver = DefaultResolver::new(workspace.to_path_buf())
        .with_recent(recent_files)
        .with_selection(current_selection);
    let budget = ContextBudget::new(budget_tokens.max(1));

    let mut items: Vec<ContextItem> = Vec::with_capacity(tagged.len());
    let mut resolved_spans: Vec<std::ops::Range<usize>> = Vec::with_capacity(tagged.len());
    for (tag, span) in tagged {
        match crate::context_resolver::handlers::resolve_tag_async(
            &tag,
            workspace,
            &resolver.recent_files,
            &resolver.current_selection,
            &budget,
        )
        .await
        {
            Ok(item) => {
                items.push(item);
                resolved_spans.push(span);
            }
            Err(err) => {
                tracing::debug!(
                    target: "context_expansion",
                    tag = %tag.label(),
                    error = %err,
                    "keeping unresolvable @tag verbatim in the prompt"
                );
            }
        }
    }

    if items.is_empty() {
        return input.to_string();
    }
    let prose = strip_spans(input, &resolved_spans);

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
