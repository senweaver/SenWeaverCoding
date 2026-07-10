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
    // Prefer the active session's workspace dir (task-local) over the process
    // CWD: the desktop runs many concurrent sessions in one process, each with a
    // different workspace, so a process-global CWD would relativize FileEdit
    // events against the wrong root and mis-attribute them in the diff panel.
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

const DIFF_CONTEXT: usize = 3;
const MAX_DIFF_CELLS: usize = 4_000_000;

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

    let before_lines: Vec<&str> = before.split('\n').collect();
    let after_lines: Vec<&str> = after.split('\n').collect();

    let body = unified_diff_body(&before_lines, &after_lines)
        .unwrap_or_else(|| fallback_diff_body(&before_lines, &after_lines));

    let mut payload = header;
    payload.push_str(&body);
    if payload.len() > MAX_DIFF_PAYLOAD {
        crate::util::truncate_string_bytes(&mut payload, MAX_DIFF_PAYLOAD);
        payload.push_str("\n... (diff truncated)\n");
    }
    Some(payload)
}

fn unified_diff_body(a: &[&str], b: &[&str]) -> Option<String> {
    let n = a.len();
    let m = b.len();
    let cap = (n + 1).saturating_mul(m + 1);
    if cap == 0 || cap > MAX_DIFF_CELLS {
        return None;
    }

    let mut dp = vec![0u32; cap];
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[idx(i, j)] = if a[i] == b[j] {
                dp[idx(i + 1, j + 1)] + 1
            } else {
                dp[idx(i + 1, j)].max(dp[idx(i, j + 1)])
            };
        }
    }

    let mut ops: Vec<(char, usize, usize)> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push((' ', i, j));
            i += 1;
            j += 1;
        } else if dp[idx(i + 1, j)] >= dp[idx(i, j + 1)] {
            ops.push(('-', i, j));
            i += 1;
        } else {
            ops.push(('+', i, j));
            j += 1;
        }
    }
    while i < n {
        ops.push(('-', i, j));
        i += 1;
    }
    while j < m {
        ops.push(('+', i, j));
        j += 1;
    }

    Some(render_hunks(a, b, &ops))
}

fn render_hunks(a: &[&str], b: &[&str], ops: &[(char, usize, usize)]) -> String {
    let total = ops.len();
    let mut include = vec![false; total];
    for (k, op) in ops.iter().enumerate() {
        if op.0 != ' ' {
            let lo = k.saturating_sub(DIFF_CONTEXT);
            let hi = (k + DIFF_CONTEXT + 1).min(total);
            for slot in include.iter_mut().take(hi).skip(lo) {
                *slot = true;
            }
        }
    }

    let mut out = String::new();
    let mut k = 0;
    while k < total {
        if !include[k] {
            k += 1;
            continue;
        }
        let start = k;
        while k < total && include[k] {
            k += 1;
        }
        let end = k;

        let mut a_start: Option<usize> = None;
        let mut b_start: Option<usize> = None;
        let mut a_count = 0usize;
        let mut b_count = 0usize;
        for op in &ops[start..end] {
            match op.0 {
                ' ' => {
                    a_start.get_or_insert(op.1);
                    b_start.get_or_insert(op.2);
                    a_count += 1;
                    b_count += 1;
                }
                '-' => {
                    a_start.get_or_insert(op.1);
                    a_count += 1;
                }
                '+' => {
                    b_start.get_or_insert(op.2);
                    b_count += 1;
                }
                _ => {}
            }
        }
        let a_s = a_start.map_or(0, |x| x + 1);
        let b_s = b_start.map_or(0, |x| x + 1);
        out.push_str(&format!("@@ -{a_s},{a_count} +{b_s},{b_count} @@\n"));
        for op in &ops[start..end] {
            let (tag, ai, bj) = *op;
            let line = if tag == '+' { b[bj] } else { a[ai] };
            out.push(tag);
            out.push_str(line);
            out.push('\n');
            if out.len() > MAX_DIFF_PAYLOAD {
                return out;
            }
        }
    }
    out
}

fn fallback_diff_body(a: &[&str], b: &[&str]) -> String {
    let mut out = format!("@@ -1,{} +1,{} @@\n", a.len(), b.len());
    for line in a {
        out.push('-');
        out.push_str(line);
        out.push('\n');
        if out.len() > MAX_DIFF_PAYLOAD {
            return out;
        }
    }
    for line in b {
        out.push('+');
        out.push_str(line);
        out.push('\n');
        if out.len() > MAX_DIFF_PAYLOAD {
            return out;
        }
    }
    out
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
    // Offload all CPU-heavy work (utf8 decode of full files, LCS line-change counting,
    // unified-diff rendering, path canonicalization) onto the blocking pool so the async
    // worker thread / agent loop is never stalled by large file edits. The send is still
    // awaited afterwards to preserve event ordering.
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
