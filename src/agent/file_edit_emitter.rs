// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use crate::agent::loop_::{DraftEvent, take_parent_draft_channel};

const MAX_DIFF_PAYLOAD: usize = 64 * 1024;
pub const WHOLE_FILE_EMIT_THRESHOLD: usize = 256 * 1024;
const DIFF_DEADLINE: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct FileEditNotice {
    pub path: PathBuf,
    pub additions: i32,
    pub deletions: i32,
    pub diff: Option<String>,
    pub edit_batch_id: Option<String>,
}

#[must_use]
pub fn relativize_for_workspace(path: &Path) -> PathBuf {
    let workspace = crate::session::current_session_context()
        .map(|ctx| PathBuf::from(ctx.workspace_dir))
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    if workspace.as_os_str().is_empty() {
        return path.to_path_buf();
    }
    let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let abs_ws = std::fs::canonicalize(&workspace).unwrap_or(workspace);
    crate::util::path_relative_to(&abs_path, &abs_ws).unwrap_or_else(|| path.to_path_buf())
}

#[must_use]
pub fn count_line_changes(before: &str, after: &str) -> (i32, i32) {
    if before == after {
        return (0, 0);
    }
    let diff = similar::TextDiff::configure()
        .timeout(DIFF_DEADLINE)
        .diff_lines(before, after);
    let mut additions = 0i32;
    let mut deletions = 0i32;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => additions += 1,
            similar::ChangeTag::Delete => deletions += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (additions, deletions)
}

#[must_use]
pub fn render_minimal_diff(rel_path: &Path, before: &str, after: &str) -> Option<String> {
    if before == after {
        return None;
    }
    if before.len() > WHOLE_FILE_EMIT_THRESHOLD || after.len() > WHOLE_FILE_EMIT_THRESHOLD {
        return Some(format!(
            "--- a/{p}\n+++ b/{p}\n@@ large file edit omitted from preview ({} -> {} bytes) @@\n",
            before.len(),
            after.len(),
            p = rel_path.display()
        ));
    }
    let mut payload = format!(
        "--- a/{}\n+++ b/{}\n",
        rel_path.display(),
        rel_path.display()
    );

    let diff = similar::TextDiff::configure()
        .timeout(DIFF_DEADLINE)
        .diff_lines(before, after);
    for group in diff.grouped_ops(3).iter() {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        let old_start = first.old_range().start;
        let old_end = last.old_range().end;
        let new_start = first.new_range().start;
        let new_end = last.new_range().end;
        payload.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            old_start + 1,
            old_end - old_start,
            new_start + 1,
            new_end - new_start,
        ));
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    similar::ChangeTag::Delete => '-',
                    similar::ChangeTag::Insert => '+',
                    similar::ChangeTag::Equal => ' ',
                };
                payload.push(sign);
                payload.push_str(change.value());
                if !change.value().ends_with('\n') {
                    payload.push('\n');
                }
                if payload.len() > MAX_DIFF_PAYLOAD {
                    crate::util::truncate_string_bytes(&mut payload, MAX_DIFF_PAYLOAD);
                    payload.push_str("\n... (diff truncated)\n");
                    return Some(payload);
                }
            }
        }
    }
    Some(payload)
}

pub async fn emit_file_edit(
    path: &Path,
    before_bytes: Option<&[u8]>,
    after_bytes: Option<&[u8]>,
    edit_batch_id: Option<String>,
) {
    let Some(tx) = take_parent_draft_channel() else {
        return;
    };
    let before_owned = before_bytes.map(<[u8]>::to_vec);
    let after_owned = after_bytes.map(<[u8]>::to_vec);
    let path_owned = path.to_path_buf();
    let built = tokio::task::spawn_blocking(move || -> Option<DraftEvent> {
        let before_text = before_owned
            .as_deref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        let after_text = after_owned
            .as_deref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        let (additions, deletions) = if before_owned.is_none() {
            let lines = after_text.split('\n').count() as i32;
            let trailing = i32::from(after_text.ends_with('\n'));
            (std::cmp::max(0, lines - trailing), 0i32)
        } else if after_owned.is_none() {
            let lines = before_text.split('\n').count() as i32;
            let trailing = i32::from(before_text.ends_with('\n'));
            (0i32, std::cmp::max(0, lines - trailing))
        } else {
            count_line_changes(&before_text, &after_text)
        };
        if additions == 0 && deletions == 0 && before_owned.is_some() && after_owned.is_some() {
            return None;
        }
        let rel = relativize_for_workspace(&path_owned);
        let diff = render_minimal_diff(&rel, &before_text, &after_text);
        Some(DraftEvent::FileEdit {
            path: rel.to_string_lossy().into_owned(),
            additions,
            deletions,
            diff,
            edit_batch_id,
        })
    })
    .await;
    if let Ok(Some(event)) = built {
        let _ = tx.send(event).await;
    }
}

pub async fn emit_file_edit_large(
    path: &Path,
    before_len: usize,
    after_len: usize,
    edit_batch_id: Option<String>,
) {
    let Some(tx) = take_parent_draft_channel() else {
        return;
    };
    let path_owned = path.to_path_buf();
    let built = tokio::task::spawn_blocking(move || -> Option<DraftEvent> {
        let rel = relativize_for_workspace(&path_owned);
        let delta = after_len as i64 - before_len as i64;
        let (additions, deletions) = if delta >= 0 {
            (delta as i32, 0i32)
        } else {
            (0i32, (-delta) as i32)
        };
        let diff = Some(format!(
            "--- a/{p}\n+++ b/{p}\n@@ large file edit omitted from preview ({before_len} -> {after_len} bytes) @@\n",
            p = rel.display()
        ));
        Some(DraftEvent::FileEdit {
            path: rel.to_string_lossy().into_owned(),
            additions,
            deletions,
            diff,
            edit_batch_id,
        })
    })
    .await;
    if let Ok(Some(event)) = built {
        let _ = tx.send(event).await;
    }
}

pub async fn emit_file_create(
    path: &Path,
    after_bytes: &[u8],
    edit_batch_id: Option<String>,
) {
    emit_file_edit(path, None, Some(after_bytes), edit_batch_id).await;
}

pub async fn emit_file_delete(
    path: &Path,
    before_bytes: &[u8],
    edit_batch_id: Option<String>,
) {
    emit_file_edit(path, Some(before_bytes), None, edit_batch_id).await;
}
