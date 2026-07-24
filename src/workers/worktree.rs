// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    pub base: PathBuf,
}

pub async fn parent_workspace_is_dirty(base: &Path) -> bool {
    crate::util::hidden_async_command("git")
        .args(["status", "--porcelain"])
        .current_dir(base)
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

pub async fn commit_worker_changes(info: &WorktreeInfo) -> Result<(), String> {
    let path = info.path.to_string_lossy().to_string();
    if !info.path.exists() {
        return Ok(());
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
    if !staged.status.success() {
        let msg = format!("sen-worker: {}", info.branch);
        let commit = crate::util::hidden_async_command("git")
            .args(["-C", &path, "commit", "-m", &msg, "--no-verify"])
            .output()
            .await
            .map_err(|e| format!("git commit failed: {e}"))?;
        if !commit.status.success() {
            return Err(String::from_utf8_lossy(&commit.stderr).trim().to_string());
        }
    }
    Ok(())
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
    let base = info.base.to_string_lossy().to_string();

    commit_worker_changes(info).await?;

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
    let ahead_count: u64 = String::from_utf8_lossy(&ahead.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    if !ahead.status.success() || ahead_count == 0 {
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
    let base = info.base.to_string_lossy().to_string();
    let path = info.path.to_string_lossy().to_string();
    let removed = crate::util::hidden_async_command("git")
        .args(["-C", &base, "worktree", "remove", "--force", &path])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if removed {
        let _ = crate::util::hidden_async_command("git")
            .args(["-C", &base, "branch", "-D", &info.branch])
            .output()
            .await;
        " (worktree and branch cleaned up)".to_string()
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
        if rel.is_empty() || rel == ".sen" || rel.starts_with(".sen/") || rel.starts_with(".git") {
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
