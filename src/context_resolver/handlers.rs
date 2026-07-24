// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::budget::ContextBudget;
use super::types::{ContextItem, ContextResolveError, ContextTag};

pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const MAX_FOLDER_ENTRIES: usize = 80;

fn read_symbol_definition(root: &Path, rel: &Path, line: u32, line_end: u32) -> Option<String> {
    if line == 0 {
        return None;
    }
    let full = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        root.join(rel)
    };
    let content = std::fs::read_to_string(&full).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let start = (line as usize).saturating_sub(1);
    if start >= lines.len() {
        return None;
    }
    let end = if line_end > line {
        (line_end as usize).min(lines.len()).min(start + 60)
    } else {
        (start + 1).min(lines.len())
    };
    Some(lines[start..end].join("\n"))
}

fn load_file_body(root: &Path, path: &PathBuf) -> Result<String, ContextResolveError> {
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
    Ok(String::from_utf8_lossy(body_slice).to_string())
}

fn finish_file_item(path: &PathBuf, body: String, budget: &ContextBudget) -> ContextItem {
    let want = crate::providers::traits::estimate_content_tokens(&body);
    let granted = budget.reserve_at_most(want);
    let final_body = if granted < want {
        clip_to_token_budget(&body, granted)
    } else {
        body
    };
    ContextItem::new(
        format!("file:{}", path.display()),
        format!("File {}", path.display()),
        final_body,
    )
    .with_source("fs")
}

pub(crate) fn budget_clip_body(body: String, budget: &ContextBudget) -> String {
    let want = crate::providers::traits::estimate_content_tokens(&body).max(1);
    let granted = budget.reserve_at_most(want);
    if granted < want {
        clip_to_token_budget(&body, granted)
    } else {
        body
    }
}

fn clip_to_token_budget(body: &str, granted: usize) -> String {
    if granted == 0 {
        return String::new();
    }
    let mut ascii_units = 0usize;
    let mut wide_tokens = 0usize;
    let mut end = 0usize;
    for (idx, ch) in body.char_indices() {
        if ch.is_ascii() {
            ascii_units += 1;
        } else {
            wide_tokens += 1;
        }
        let tokens = ascii_units * 10 / 34 + wide_tokens;
        if tokens > granted {
            break;
        }
        end = idx + ch.len_utf8();
    }
    body[..end].to_string()
}

pub fn resolve_file(
    root: &Path,
    path: &PathBuf,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let body = load_file_body(root, path)?;
    Ok(finish_file_item(path, body, budget))
}

fn load_folder_listing(root: &Path, path: &PathBuf) -> Result<String, ContextResolveError> {
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
    Ok(lines.join("\n"))
}

fn finish_folder_item(path: &PathBuf, body: String, budget: &ContextBudget) -> ContextItem {
    let final_body = budget_clip_body(body, budget);
    ContextItem::new(
        format!("folder:{}", path.display()),
        format!("Folder listing: {}", path.display()),
        final_body,
    )
    .with_source("fs")
}

pub fn resolve_folder(
    root: &Path,
    path: &PathBuf,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let body = load_folder_listing(root, path)?;
    Ok(finish_folder_item(path, body, budget))
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
    root: &Path,
    name: &str,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let mut body = String::new();
    {
        if let crate::code_intel::symbol_graph::incremental::WriterAvailability::Ready(writer) =
            crate::code_intel::symbol_graph::incremental::get_writer_nonblocking(root)
        {
            let graph_lock = writer.graph();
            let graph = graph_lock.read();
            let name_lc = name.to_ascii_lowercase();
            let mut matches: Vec<&crate::code_intel::symbol_graph::SymbolEntry> = graph
                .symbols
                .iter()
                .filter(|sym| sym.id.name.to_ascii_lowercase().contains(&name_lc))
                .collect();
            matches.sort_by_key(|sym| sym.id.name.to_ascii_lowercase() != name_lc);
            for (idx, sym) in matches.iter().take(8).enumerate() {
                let _ = writeln!(
                    body,
                    "{} ({}) @ {}:{}",
                    sym.id.name,
                    sym.kind,
                    sym.id.file.display(),
                    sym.id.line
                );
                if idx == 0 {
                    if let Some(def) =
                        read_symbol_definition(root, &sym.id.file, sym.id.line, sym.line_end)
                    {
                        let _ = writeln!(body, "```\n{def}\n```");
                    }
                }
            }
        }
    }
    if body.is_empty() {
        body = format!(
            "<symbol> {name} </symbol>\n(No symbol_graph hit; use code_outline / content_search.)"
        );
    }
    let final_body = budget_clip_body(body, budget);
    Ok(ContextItem::new(
        format!("symbol:{name}"),
        format!("Symbol reference: {name}"),
        final_body,
    )
    .with_source("symbol_graph"))
}

pub fn resolve_diff(
    root: &Path,
    range: &str,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let range = range.trim();
    if range.starts_with('-') {
        return Ok(ContextItem::new(
            format!("diff:{range}"),
            format!("Git diff: {range}"),
            format!("<diff> {range} </diff>\n(rejected: range must not start with '-')"),
        )
        .with_source("git"));
    }
    let mut cmd = crate::util::hidden_sync_command("git");
    cmd.current_dir(root);
    let args: Vec<&str> = if range.is_empty() {
        vec!["diff", "-U3"]
    } else {
        vec!["diff", "-U3", range]
    };
    cmd.args(&args);
    let output = cmd.output();
    let body = match output {
        Ok(o) if o.status.success() || !o.stdout.is_empty() => {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            if text.trim().is_empty() {
                format!("<diff> {range} </diff>\n(empty diff)")
            } else {
                text
            }
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            format!("<diff> {range} </diff>\n(git diff failed: {err})")
        }
        Err(e) => format!("<diff> {range} </diff>\n(git unavailable: {e})"),
    };
    let final_body = budget_clip_body(body, budget);
    Ok(ContextItem::new(
        format!("diff:{range}"),
        format!("Git diff: {range}"),
        final_body,
    )
    .with_source("git"))
}

pub fn resolve_test(
    name: &str,
    _budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    Ok(ContextItem::new(
        format!("test:{name}"),
        format!("Test reference: {name}"),
        format!(
            "<test> {name} </test>\n(Not inlined; run the project check/test command if the workspace allows tests.)"
        ),
    )
    .with_source("reference"))
}

pub fn resolve_doc(
    root: &Path,
    name: &str,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let needle = name.trim();
    let mut found: Option<PathBuf> = None;
    let candidates = [
        root.join(needle),
        root.join("docs").join(needle),
        root.join("README.md"),
        root.join(format!("{needle}.md")),
        root.join("docs").join(format!("{needle}.md")),
    ];
    for c in candidates {
        if c.is_file() {
            found = Some(c);
            break;
        }
    }
    let body = if let Some(path) = found {
        match load_file_body(root, &path) {
            Ok(b) => b,
            Err(_) => format!("<doc> {name} </doc>\n(file unreadable)"),
        }
    } else {
        format!("<doc> {name} </doc>\n(No matching doc file found under workspace.)")
    };
    let final_body = budget_clip_body(body, budget);
    Ok(ContextItem::new(
        format!("doc:{name}"),
        format!("Documentation: {name}"),
        final_body,
    )
    .with_source("fs"))
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
        ContextTag::Diff(r) => resolve_diff(root, r, budget),
        ContextTag::Test(t) => resolve_test(t, budget),
        ContextTag::Doc(d) => resolve_doc(root, d, budget),
        ContextTag::Recent => resolve_recent(recents, budget),
        ContextTag::Selection => resolve_selection(selection, budget),
        ContextTag::Codebase(q) => super::codebase::resolve_codebase(root, q, budget),
        ContextTag::Problems => Ok(ContextItem::new(
            "problems",
            "Problems",
            "(diagnostics are resolved asynchronously)",
        )
        .with_source("lsp")),
    }
}

pub async fn resolve_tag_async(
    tag: &ContextTag,
    root: &Path,
    recents: &[PathBuf],
    selection: &str,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    match tag {
        ContextTag::File(p) => {
            let root_owned = root.to_path_buf();
            let path_owned = p.clone();
            let body = tokio::task::spawn_blocking(move || {
                load_file_body(&root_owned, &path_owned)
            })
            .await
            .map_err(|e| ContextResolveError::Io(std::io::Error::other(e)))??;
            Ok(finish_file_item(p, body, budget))
        }
        ContextTag::Folder(p) => {
            let root_owned = root.to_path_buf();
            let path_owned = p.clone();
            let body = tokio::task::spawn_blocking(move || {
                load_folder_listing(&root_owned, &path_owned)
            })
            .await
            .map_err(|e| ContextResolveError::Io(std::io::Error::other(e)))??;
            Ok(finish_folder_item(p, body, budget))
        }
        ContextTag::Codebase(q) => {
            if let Some(source) = crate::agent::loop_::services::rag_source(root) {
                let hits = source.retrieve(q, 8).await;
                if !hits.is_empty() {
                    let mut body = String::with_capacity(1024);
                    for (i, hit) in hits.iter().enumerate() {
                        let rel = crate::util::path_relative_to(&hit.path, root)
                            .unwrap_or_else(|| hit.path.clone());
                        let rel_str = rel.to_string_lossy().replace('\\', "/");
                        body.push_str(&format!("{}. {}:{}\n", i + 1, rel_str, hit.line));
                        for line in hit.snippet.trim_end().lines().take(6) {
                            body.push_str("    ");
                            body.push_str(line);
                            body.push('\n');
                        }
                        body.push('\n');
                    }
                    return Ok(super::codebase::finish_codebase_item(q, body, budget));
                }
            }
            let root_owned = root.to_path_buf();
            let query_owned = q.clone();
            let body = tokio::task::spawn_blocking(move || {
                super::codebase::collect_codebase_body(&root_owned, &query_owned)
            })
            .await
            .map_err(|e| ContextResolveError::Io(std::io::Error::other(e)))??;
            Ok(super::codebase::finish_codebase_item(q, body, budget))
        }
        ContextTag::Problems => resolve_problems(root, budget).await,
        other => resolve_tag(other, root, recents, selection, budget),
    }
}

async fn resolve_problems(
    root: &Path,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let Some(svc) = crate::services::try_get_services() else {
        return Err(ContextResolveError::NotFound {
            tag: "problems".to_string(),
            reason: "services unavailable".to_string(),
        });
    };
    let all = svc.lsp.get_all_diagnostics().await;
    let mut lines: Vec<String> = Vec::new();
    let mut errors = 0usize;
    let mut warnings = 0usize;
    for (path, diags) in &all {
        let rel = crate::util::path_relative_to(path, root).unwrap_or_else(|| path.clone());
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        for d in diags {
            let sev = format!("{:?}", d.severity).to_ascii_lowercase();
            if sev.contains("error") {
                errors += 1;
            } else if sev.contains("warn") {
                warnings += 1;
            }
            if lines.len() < 100 {
                lines.push(format!(
                    "{}:{}: [{}] {}",
                    rel_str,
                    d.range.start_line + 1,
                    sev,
                    d.message.lines().next().unwrap_or("").trim()
                ));
            }
        }
    }
    let body = if lines.is_empty() {
        "No LSP diagnostics reported for the workspace.".to_string()
    } else {
        format!(
            "{errors} error(s), {warnings} warning(s):\n{}",
            lines.join("\n")
        )
    };
    let final_body = budget_clip_body(body, budget);
    Ok(ContextItem::new("problems", "Workspace problems", final_body).with_source("lsp"))
}
