// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Per-tag resolution handlers.
//!
//! Each public function takes a reference to the workspace root
//! plus the tag payload and returns a [`ContextItem`].  Handlers are
//! small on purpose so they are easy to unit-test without pulling
//! heavier subsystems (SymbolGraph / Tantivy / git) into the hot path.

use std::path::{Path, PathBuf};

use super::budget::ContextBudget;
use super::types::{ContextItem, ContextResolveError, ContextTag};

pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const MAX_FOLDER_ENTRIES: usize = 80;

pub fn resolve_file(
    root: &Path,
    path: &PathBuf,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let full = if path.is_absolute() {
        path.clone()
    } else {
        root.join(path)
    };
    let bytes = std::fs::read(&full).map_err(|e| ContextResolveError::NotFound {
        tag: format!("file:{}", path.display()),
        reason: e.to_string(),
    })?;
    let body_slice = if bytes.len() > MAX_FILE_BYTES {
        &bytes[..MAX_FILE_BYTES]
    } else {
        &bytes[..]
    };
    let body = String::from_utf8_lossy(body_slice).to_string();
    let want = body.len() / 4;
    let granted = budget.reserve_at_most(want);
    let final_body = if granted < want {

        let take_chars = granted * 4;
        take_prefix_by_chars(&body, take_chars)
    } else {
        body
    };
    let item = ContextItem::new(
        format!("file:{}", path.display()),
        format!("File {}", path.display()),
        final_body,
    )
    .with_source("fs");
    Ok(item)
}

pub fn resolve_folder(
    root: &Path,
    path: &PathBuf,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let full = if path.is_absolute() {
        path.clone()
    } else {
        root.join(path)
    };
    let entries = std::fs::read_dir(&full).map_err(|e| ContextResolveError::NotFound {
        tag: format!("folder:{}", path.display()),
        reason: e.to_string(),
    })?;
    let mut lines = Vec::new();
    for (idx, entry) in entries.enumerate() {
        if idx >= MAX_FOLDER_ENTRIES {
            lines.push("… (truncated)".into());
            break;
        }
        match entry {
            Ok(e) => lines.push(format!("- {}", e.file_name().to_string_lossy())),
            Err(_) => continue,
        }
    }
    let body = lines.join("\n");
    let want = body.len() / 4;
    let granted = budget.reserve_at_most(want);
    let final_body = if granted < want {
        take_prefix_by_chars(&body, granted * 4)
    } else {
        body
    };
    Ok(ContextItem::new(
        format!("folder:{}", path.display()),
        format!("Folder listing: {}", path.display()),
        final_body,
    )
    .with_source("fs"))
}

pub fn resolve_url(url: &str, _budget: &ContextBudget) -> Result<ContextItem, ContextResolveError> {

    Ok(ContextItem::new(
        format!("url:{url}"),
        format!("Web reference: {url}"),
        format!("<url> {url} </url>\n(Use web_fetch to retrieve the content.)"),
    )
    .with_source("reference"))
}

pub fn resolve_symbol(
    _root: &Path,
    name: &str,
    _budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {

    Ok(ContextItem::new(
        format!("symbol:{name}"),
        format!("Symbol reference: {name}"),
        format!(
            "<symbol> {name} </symbol>\n(Use `code_grep` or `code_outline` to locate the \
             definition in the current workspace.)"
        ),
    )
    .with_source("reference"))
}

pub fn resolve_diff(
    range: &str,
    _budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    Ok(ContextItem::new(
        format!("diff:{range}"),
        format!("Git diff: {range}"),
        format!("<diff> {range} </diff>\n(Use `git diff {range}` or the `git_status` tool.)"),
    )
    .with_source("reference"))
}

pub fn resolve_test(
    name: &str,
    _budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    Ok(ContextItem::new(
        format!("test:{name}"),
        format!("Test reference: {name}"),
        format!("<test> {name} </test>\n(Use `cargo test {name}` or `node --test` to run.)"),
    )
    .with_source("reference"))
}

pub fn resolve_doc(
    name: &str,
    _budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    Ok(ContextItem::new(
        format!("doc:{name}"),
        format!("Documentation: {name}"),
        format!("<doc> {name} </doc>"),
    )
    .with_source("reference"))
}

pub fn resolve_recent(
    recents: &[PathBuf],
    _budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let body = recents
        .iter()
        .take(5)
        .map(|p| format!("- {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ContextItem::new(
        "recent",
        "Recently edited files",
        if body.is_empty() {
            "(no recent edits)".to_string()
        } else {
            body
        },
    )
    .with_source("session"))
}

pub fn resolve_selection(
    selection: &str,
    _budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    Ok(
        ContextItem::new("selection", "Current selection", selection.to_string())
            .with_source("surface"),
    )
}

fn take_prefix_by_chars(s: &str, chars: usize) -> String {
    s.chars().take(chars).collect()
}

pub(crate) fn take_prefix_by_chars_public(s: &str, chars: usize) -> String {
    take_prefix_by_chars(s, chars)
}

pub fn resolve_tag(
    tag: &ContextTag,
    root: &Path,
    recents: &[PathBuf],
    selection: &str,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    match tag {
        ContextTag::File(p) => resolve_file(root, p, budget),
        ContextTag::Folder(p) => resolve_folder(root, p, budget),
        ContextTag::Url(u) => resolve_url(u, budget),
        ContextTag::Symbol(s) => resolve_symbol(root, s, budget),
        ContextTag::Diff(r) => resolve_diff(r, budget),
        ContextTag::Test(t) => resolve_test(t, budget),
        ContextTag::Doc(d) => resolve_doc(d, budget),
        ContextTag::Recent => resolve_recent(recents, budget),
        ContextTag::Selection => resolve_selection(selection, budget),
        ContextTag::Codebase(q) => super::codebase::resolve_codebase(root, q, budget),
    }
}
