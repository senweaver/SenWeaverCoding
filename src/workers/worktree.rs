// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

#[derive(Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    pub base: PathBuf,
}

static BASE_MERGE_LOCKS: OnceLock<dashmap::DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>> =
    OnceLock::new();

pub fn base_merge_lock(base: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let map = BASE_MERGE_LOCKS.get_or_init(dashmap::DashMap::new);
    let key = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    map.entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

pub fn is_internal_repo_path(rel: &str) -> bool {
    let rel = rel.trim();
    rel.is_empty()
        || rel == ".sen"
        || rel.starts_with(".sen/")
        || rel.starts_with(".sen\\")
        || rel == ".git"
        || rel.starts_with(".git/")
        || rel.starts_with(".git\\")
}

fn porcelain_entry_is_internal(line: &str) -> bool {
    let line = line.trim_end_matches('\r');
    if line.trim().is_empty() {
        return true;
    }
    if line.len() < 4 {
        return false;
    }
    let status: String = line.chars().take(2).collect();
    let rest = &line[3..];
    let is_rename = status.contains('R') || status.contains('C');
    let (first, second) = if is_rename {
        match rest.split_once(" -> ") {
            Some((a, b)) => (a, Some(b)),
            None => (rest, None),
        }
    } else {
        (rest, None)
    };
    for raw in std::iter::once(first).chain(second) {
        let rel = unquote_git_path(raw);
        if !is_internal_repo_path(&rel) {
            return false;
        }
    }
    true
}

pub fn porcelain_has_real_changes(stdout: &str) -> bool {
    stdout.lines().any(|line| !porcelain_entry_is_internal(line))
}

pub async fn parent_workspace_is_dirty(base: &Path) -> Result<bool, String> {
    if !base.join(".git").exists() {
        return Ok(false);
    }
    let out = crate::util::hidden_async_command("git")
        .args(["status", "--porcelain"])
        .current_dir(base)
        .output()
        .await
        .map_err(|e| format!("git status failed to spawn: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err("git status exited with a non-zero status".to_string());
        }
        return Err(stderr);
    }
    Ok(porcelain_has_real_changes(&String::from_utf8_lossy(&out.stdout)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeCommit {
    Committed,
    NoChanges,
}

pub struct WorktreeSalvage {
    pub note: String,
    pub retained: bool,
}

pub async fn commit_worker_changes(info: &WorktreeInfo) -> Result<WorktreeCommit, String> {
    let path = info.path.to_string_lossy().to_string();
    if !info.path.exists() {
        return Ok(WorktreeCommit::NoChanges);
    }
    let add = crate::util::hidden_async_command("git")
        .args(["-C", &path, "add", "-A"])
        .output()
        .await
        .map_err(|e| format!("git add failed: {e}"))?;
    if !add.status.success() {
        return Err(String::from_utf8_lossy(&add.stderr).trim().to_string());
    }
    let staged = crate::util::hidden_async_command("git")
        .args(["-C", &path, "diff", "--cached", "--quiet"])
        .output()
        .await
        .map_err(|e| format!("git diff --cached failed: {e}"))?;
    if staged.status.success() {
        return Ok(WorktreeCommit::NoChanges);
    }
    let msg = format!("sen-worker: {}", info.branch);
    let commit = crate::util::hidden_async_command("git")
        .args(["-C", &path, "commit", "-m", &msg, "--no-verify"])
        .output()
        .await
        .map_err(|e| format!("git commit failed: {e}"))?;
    if !commit.status.success() {
        return Err(String::from_utf8_lossy(&commit.stderr).trim().to_string());
    }
    Ok(WorktreeCommit::Committed)
}

pub async fn salvage_worktree(info: &WorktreeInfo) -> WorktreeSalvage {
    if crate::workers::overlay::is_overlay_info(info) {
        return crate::workers::overlay::salvage_worker_overlay(info).await;
    }
    match commit_worker_changes(info).await {
        Ok(WorktreeCommit::Committed) => {
            let note = remove_worktree_keep_branch(info).await;
            WorktreeSalvage {
                note: format!("committed: work saved on branch `{}`{note}", info.branch),
                retained: false,
            }
        }
        Ok(WorktreeCommit::NoChanges) => {
            let note = remove_worktree_keep_branch(info).await;
            WorktreeSalvage {
                note: format!("no_changes: nothing to commit{note}"),
                retained: false,
            }
        }
        Err(err) => WorktreeSalvage {
            note: format!(
                "retained_uncommitted: commit failed ({err}); worktree kept at `{}` on branch \
                 `{}` so the uncommitted work can be recovered manually",
                info.path.display(),
                info.branch
            ),
            retained: true,
        },
    }
}

pub async fn remove_worktree_keep_branch(info: &WorktreeInfo) -> String {
    let base = info.base.to_string_lossy().to_string();
    let path = info.path.to_string_lossy().to_string();
    let removed = crate::util::hidden_async_command("git")
        .args(["-C", &base, "worktree", "remove", "--force", &path])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if removed {
        " (worktree removed, branch kept)".to_string()
    } else {
        let _ = crate::util::hidden_async_command("git")
            .args(["-C", &base, "worktree", "prune"])
            .output()
            .await;
        " (worktree removal deferred to prune, branch kept)".to_string()
    }
}

pub async fn commit_and_merge_worker(info: &WorktreeInfo) -> Result<String, String> {
    if crate::workers::overlay::is_overlay_info(info) {
        return crate::workers::overlay::merge_worker_overlay(info).await;
    }
    let base = info.base.to_string_lossy().to_string();

    if let Err(err) = commit_worker_changes(info).await {
        return Err(format!(
            "commit failed ({err}); worktree kept at `{}` on branch `{}` so the uncommitted \
             work can be recovered manually",
            info.path.display(),
            info.branch
        ));
    }

    let ahead = crate::util::hidden_async_command("git")
        .args([
            "-C",
            &base,
            "rev-list",
            "--count",
            &format!("HEAD..{}", info.branch),
        ])
        .output()
        .await
        .map_err(|e| format!("git rev-list failed: {e}"))?;
    if !ahead.status.success() {
        let stderr = String::from_utf8_lossy(&ahead.stderr).trim().to_string();
        return Err(format!(
            "git rev-list failed ({stderr}); worktree kept at `{}` on branch `{}` so the \
             committed work is preserved",
            info.path.display(),
            info.branch
        ));
    }
    let ahead_stdout = String::from_utf8_lossy(&ahead.stdout).trim().to_string();
    let ahead_count: u64 = match ahead_stdout.parse() {
        Ok(n) => n,
        Err(_) => {
            return Err(format!(
                "git rev-list returned unparseable output `{ahead_stdout}`; worktree kept at \
                 `{}` on branch `{}` so the committed work is preserved",
                info.path.display(),
                info.branch
            ));
        }
    };
    if ahead_count == 0 {
        let cleanup = remove_worker_worktree(info).await;
        return Ok(format!("no commits ahead of parent; merge skipped{cleanup}"));
    }

    if let Some(conflicts) = merge_tree_conflicts(&base, &info.branch).await {
        if !conflicts.is_empty() {
            let note = remove_worktree_keep_branch(info).await;
            return Err(format!(
                "predicted merge conflict in: {} (branch `{}` preserved{note} — resolve with \
                 `git merge {}`)",
                conflicts.replace('\n', ", "),
                info.branch,
                info.branch
            ));
        }
    }

    let merge = crate::util::hidden_async_command("git")
        .args(["-C", &base, "merge", "--no-edit", "--no-ff", &info.branch])
        .output()
        .await
        .map_err(|e| format!("git merge failed to spawn: {e}"))?;
    if merge.status.success() {
        let cleanup = remove_worker_worktree(info).await;
        return Ok(format!("committed and merged into parent workspace{cleanup}"));
    }

    let stderr = String::from_utf8_lossy(&merge.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&merge.stdout).trim().to_string();
    let conflicts = crate::util::hidden_async_command("git")
        .args(["-C", &base, "diff", "--name-only", "--diff-filter=U"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let _ = crate::util::hidden_async_command("git")
        .args(["-C", &base, "merge", "--abort"])
        .output()
        .await;

    let mut detail = String::new();
    if !conflicts.is_empty() {
        detail.push_str("conflict paths: ");
        detail.push_str(&conflicts.replace('\n', ", "));
        detail.push_str("; ");
    }
    if !stderr.is_empty() {
        detail.push_str(&stderr);
    } else if !stdout.is_empty() {
        detail.push_str(&stdout);
    } else {
        detail.push_str("merge conflict; aborted and left worker branch intact");
    }
    let note = remove_worktree_keep_branch(info).await;
    detail.push_str(&format!(
        " (branch `{}` preserved{note} — resolve with `git merge {}`)",
        info.branch, info.branch
    ));
    Err(detail)
}

pub async fn merge_tree_conflicts(base: &str, branch: &str) -> Option<String> {
    let out = crate::util::hidden_async_command("git")
        .args([
            "-C",
            base,
            "merge-tree",
            "--write-tree",
            "--name-only",
            "HEAD",
            branch,
        ])
        .output()
        .await
        .ok()?;
    if out.status.success() {
        return Some(String::new());
    }
    let code = out.status.code().unwrap_or(0);
    if code != 1 {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let conflicts: Vec<&str> = stdout
        .lines()
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .collect();
    Some(conflicts.join("\n"))
}

pub async fn remove_worker_worktree(info: &WorktreeInfo) -> String {
    if crate::workers::overlay::is_overlay_info(info) {
        return crate::workers::overlay::remove_worker_overlay(info).await;
    }
    let base = info.base.to_string_lossy().to_string();
    let path = info.path.to_string_lossy().to_string();
    let removed = crate::util::hidden_async_command("git")
        .args(["-C", &base, "worktree", "remove", "--force", &path])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if removed {
        let deleted = crate::util::hidden_async_command("git")
            .args(["-C", &base, "branch", "-d", &info.branch])
            .output()
            .await;
        match deleted {
            Ok(o) if o.status.success() => " (worktree and branch cleaned up)".to_string(),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                let reason = if stderr.is_empty() {
                    "git branch -d refused to delete it".to_string()
                } else {
                    stderr
                };
                format!(
                    " (worktree removed; branch `{}` not deleted: {reason})",
                    info.branch
                )
            }
            Err(e) => format!(
                " (worktree removed; branch `{}` not deleted: {e})",
                info.branch
            ),
        }
    } else {
        let _ = crate::util::hidden_async_command("git")
            .args(["-C", &base, "worktree", "prune"])
            .output()
            .await;
        " (merged; worktree cleanup deferred to prune)".to_string()
    }
}

pub async fn create_named_worktree(
    base: &Path,
    branch: &str,
    dir_name: &str,
) -> Result<WorktreeInfo, String> {
    let inside = crate::util::hidden_async_command("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(base)
        .output()
        .await
        .map_err(|e| format!("git not available: {e}"))?;
    if !inside.status.success() {
        return Err("not a git repository".to_string());
    }

    let dirty_status = crate::util::hidden_async_command("git")
        .args(["status", "--porcelain"])
        .current_dir(base)
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let worktrees_dir = base.join(".sen").join("worktrees");
    if let Err(e) = tokio::fs::create_dir_all(&worktrees_dir).await {
        return Err(format!("failed to create worktrees dir: {e}"));
    }
    let path = worktrees_dir.join(dir_name);
    let path_str = path.to_string_lossy().to_string();

    let output = crate::util::hidden_async_command("git")
        .args(["worktree", "add", "-b", branch, &path_str, "HEAD"])
        .current_dir(base)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("git worktree add failed to spawn: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    if !dirty_status.trim().is_empty() {
        replicate_uncommitted_changes(base, &path, &path_str, &dirty_status).await;
    }

    Ok(WorktreeInfo {
        path,
        branch: branch.to_string(),
        base: base.to_path_buf(),
    })
}

pub async fn create_worker_worktree(base: &Path, idx: usize) -> Result<WorktreeInfo, String> {
    if !base.join(".git").exists() {
        return crate::workers::overlay::create_worker_overlay(base, idx).await;
    }
    let batch_id = uuid::Uuid::new_v4().simple().to_string();
    let short_id = &batch_id[..12.min(batch_id.len())];
    let branch = format!("sen-worker/{short_id}-{idx}");
    let dir_name = format!("{short_id}-{idx}");
    create_named_worktree(base, &branch, &dir_name).await
}

async fn replicate_uncommitted_changes(base: &Path, path: &Path, path_str: &str, status: &str) {
    let diff = crate::util::hidden_async_command("git")
        .args(["diff", "HEAD", "--binary"])
        .current_dir(base)
        .output()
        .await;
    if let Ok(d) = diff {
        if d.status.success() && !d.stdout.is_empty() {
            let patch_path = path.join(".sen-uncommitted.patch");
            if tokio::fs::write(&patch_path, &d.stdout).await.is_ok() {
                let applied = crate::util::hidden_async_command("git")
                    .args([
                        "-C",
                        path_str,
                        "apply",
                        "--whitespace=nowarn",
                        &patch_path.to_string_lossy(),
                    ])
                    .output()
                    .await
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                let _ = tokio::fs::remove_file(&patch_path).await;
                if !applied {
                    tracing::warn!(
                        target: "workers.worktree",
                        "could not replay parent uncommitted diff into worker worktree; \
                         it starts from HEAD for tracked files"
                    );
                }
            }
        }
    }

    for line in status.lines() {
        let Some(raw) = line.strip_prefix("?? ") else {
            continue;
        };
        let rel = unquote_git_path(raw);
        if is_internal_repo_path(&rel) {
            continue;
        }
        let is_dir = rel.ends_with('/');
        let rel_clean = rel.trim_end_matches('/');
        let src = base.join(rel_clean);
        let dst = path.join(rel_clean);
        if is_dir {
            let src_owned = src.clone();
            let dst_owned = dst.clone();
            let copied =
                tokio::task::spawn_blocking(move || copy_dir_recursive_sync(&src_owned, &dst_owned))
                    .await;
            if copied.is_err() {
                tracing::debug!(
                    target: "workers.worktree",
                    dir = %rel_clean,
                    "could not copy untracked directory into worker worktree"
                );
            }
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = tokio::fs::copy(&src, &dst).await {
            tracing::debug!(
                target: "workers.worktree",
                file = %rel_clean,
                error = %e,
                "could not copy untracked file into worker worktree"
            );
        }
    }
}

fn unquote_git_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
        return trimmed.to_string();
    }
    let inner = trimmed[1..trimmed.len() - 1].as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut i = 0usize;
    while i < inner.len() {
        let b = inner[i];
        if b != b'\\' {
            out.push(b);
            i += 1;
            continue;
        }
        i += 1;
        if i >= inner.len() {
            break;
        }
        match inner[i] {
            b'n' => {
                out.push(b'\n');
                i += 1;
            }
            b't' => {
                out.push(b'\t');
                i += 1;
            }
            b'r' => {
                out.push(b'\r');
                i += 1;
            }
            b'\\' => {
                out.push(b'\\');
                i += 1;
            }
            b'"' => {
                out.push(b'"');
                i += 1;
            }
            b'0'..=b'7' => {
                let mut val: u32 = 0;
                let mut n = 0usize;
                while n < 3 && i < inner.len() && (b'0'..=b'7').contains(&inner[i]) {
                    val = val * 8 + u32::from(inner[i] - b'0');
                    i += 1;
                    n += 1;
                }
                out.push((val & 0xFF) as u8);
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn copy_dir_recursive_sync(src: &Path, dst: &Path) {
    let Ok(rd) = std::fs::read_dir(src) else {
        return;
    };
    let _ = std::fs::create_dir_all(dst);
    for entry in rd.flatten() {
        let p = entry.path();
        let name = entry.file_name();
        if name == ".git" || name == ".sen" {
            continue;
        }
        let d = dst.join(&name);
        if p.is_dir() {
            copy_dir_recursive_sync(&p, &d);
        } else {
            let _ = std::fs::copy(&p, &d);
        }
    }
}

pub async fn worktree_change_report(info: &WorktreeInfo) -> String {
    if crate::workers::overlay::is_overlay_info(info) {
        return crate::workers::overlay::overlay_change_report(info).await;
    }
    let output = crate::util::hidden_async_command("git")
        .args(["-C", &info.path.to_string_lossy(), "status", "--short"])
        .current_dir(&info.base)
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                "(no file changes)".to_string()
            } else {
                truncate_chars(trimmed, 1_500)
            }
        }
        _ => "(unable to read worktree status)".to_string(),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
