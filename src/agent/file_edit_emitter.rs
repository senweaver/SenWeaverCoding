// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use crate::agent::loop_::{DraftEvent, take_parent_draft_channel};

const MAX_DIFF_PAYLOAD: usize = 64 * 1024;

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
    let workspace = std::env::current_dir().ok().unwrap_or_default();
    if workspace.as_os_str().is_empty() {
        return path.to_path_buf();
    }
    let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let abs_ws = std::fs::canonicalize(&workspace).unwrap_or(workspace);
    abs_path
        .strip_prefix(&abs_ws)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

#[must_use]
pub fn count_line_changes(before: &str, after: &str) -> (i32, i32) {
    if before == after {
        return (0, 0);
    }
    let before_lines: Vec<&str> = before.split('\n').collect();
    let after_lines: Vec<&str> = after.split('\n').collect();
    let lcs = lcs_length(&before_lines, &after_lines);
    let deletions = before_lines.len().saturating_sub(lcs) as i32;
    let additions = after_lines.len().saturating_sub(lcs) as i32;
    (additions, deletions)
}

fn lcs_length(a: &[&str], b: &[&str]) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let n = a.len();
    let m = b.len();
    let cap = (n + 1).saturating_mul(m + 1);
    if cap > 4_000_000 {
        return std::cmp::min(n, m);
    }
    let mut prev = vec![0usize; m + 1];
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        for j in 1..=m {
            if a[i - 1] == b[j - 1] {
                curr[j] = prev[j - 1] + 1;
            } else {
                curr[j] = std::cmp::max(prev[j], curr[j - 1]);
            }
        }
        std::mem::swap(&mut prev, &mut curr);
        for slot in curr.iter_mut() {
            *slot = 0;
        }
    }
    prev[m]
}

#[must_use]
pub fn render_minimal_diff(rel_path: &Path, before: &str, after: &str) -> Option<String> {
    if before == after {
        return None;
    }
    let header = format!(
        "--- a/{}\n+++ b/{}\n",
        rel_path.display(),
        rel_path.display()
    );
    let mut payload = header;
    let mut buf = String::new();
    for line in before.split('\n') {
        buf.push('-');
        buf.push_str(line);
        buf.push('\n');
        if buf.len() > MAX_DIFF_PAYLOAD {
            break;
        }
    }
    for line in after.split('\n') {
        buf.push('+');
        buf.push_str(line);
        buf.push('\n');
        if buf.len() > MAX_DIFF_PAYLOAD {
            break;
        }
    }
    payload.push_str(&buf);
    if payload.len() > MAX_DIFF_PAYLOAD {
        payload.truncate(MAX_DIFF_PAYLOAD);
        payload.push_str("\n... (diff truncated)\n");
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
    let before_text = before_bytes
        .map(String::from_utf8_lossy)
        .unwrap_or_default()
        .into_owned();
    let after_text = after_bytes
        .map(String::from_utf8_lossy)
        .unwrap_or_default()
        .into_owned();
    let (additions, deletions) = if before_bytes.is_none() {
        let lines = after_text.split('\n').count() as i32;
        let trailing = if after_text.ends_with('\n') { 1 } else { 0 };
        (std::cmp::max(0, lines - trailing), 0i32)
    } else if after_bytes.is_none() {
        let lines = before_text.split('\n').count() as i32;
        let trailing = if before_text.ends_with('\n') { 1 } else { 0 };
        (0i32, std::cmp::max(0, lines - trailing))
    } else {
        count_line_changes(&before_text, &after_text)
    };
    if additions == 0 && deletions == 0 && before_bytes.is_some() && after_bytes.is_some() {
        return;
    }
    let rel = relativize_for_workspace(path);
    let diff = render_minimal_diff(&rel, &before_text, &after_text);
    let event = DraftEvent::FileEdit {
        path: rel.to_string_lossy().into_owned(),
        additions,
        deletions,
        diff,
        edit_batch_id,
    };
    let _ = tx.send(event).await;
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
