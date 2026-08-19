// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::worktree::{WorktreeInfo, WorktreeSalvage};

const OVERLAY_DIR_NAME: &str = "worktrees-cow";
const MANIFEST_FILE: &str = "overlay-manifest.json";
const MAX_SNAPSHOT_FILE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_SNAPSHOT_FILES: usize = 50_000;
const MAX_WALK_DEPTH: usize = 32;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".sen",
    "node_modules",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
    ".next",
    ".turbo",
    ".idea",
    ".vscode",
];

#[derive(Debug, Default, Serialize, Deserialize)]
struct OverlayManifest {
    #[serde(default)]
    entries: BTreeMap<String, String>,
}

#[must_use]
pub fn is_overlay_info(info: &WorktreeInfo) -> bool {
    info.branch.is_empty()
}

fn control_dir(info: &WorktreeInfo) -> PathBuf {
    info.path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| info.path.clone())
}

fn manifest_path(info: &WorktreeInfo) -> PathBuf {
    control_dir(info).join(MANIFEST_FILE)
}

fn rel_key(root: &Path, abs: &Path) -> Option<String> {
    let stripped = abs.strip_prefix(root).ok()?;
    match stripped.to_str() {
        Some(text) => Some(text.replace('\\', "/")),
        None => {
            tracing::warn!(
                target: "workers.overlay",
                path = %abs.display(),
                "skipping file with non-UTF-8 name (cannot be tracked losslessly in overlay manifest)"
            );
            None
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    crate::apply_model::edit_op::sha256_hex(bytes)
}

#[cfg(windows)]
fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    std::fs::symlink_metadata(path)
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_reparse_point(_path: &Path) -> bool {
    false
}

fn walk_snapshot_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let ignore = crate::code_intel::search::build_gitignore_set(root);
    let mut files = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() || is_reparse_point(&path) {
                continue;
            }
            if file_type.is_dir() {
                if depth >= MAX_WALK_DEPTH {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if SKIP_DIRS.contains(&name) {
                    continue;
                }
                if let Some(set) = ignore.as_ref() {
                    if crate::code_intel::search::path_is_gitignored(set, root, &path) {
                        continue;
                    }
                }
                stack.push((path, depth + 1));
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            if let Some(set) = ignore.as_ref() {
                if crate::code_intel::search::path_is_gitignored(set, root, &path) {
                    continue;
                }
            }
            if entry
                .metadata()
                .map(|m| m.len() > MAX_SNAPSHOT_FILE_BYTES)
                .unwrap_or(true)
            {
                continue;
            }
            files.push(path);
            if files.len() > MAX_SNAPSHOT_FILES {
                return Err(format!(
                    "workspace has more than {MAX_SNAPSHOT_FILES} snapshot-eligible files; overlay isolation refused (check ignore rules)"
                ));
            }
        }
    }
    Ok(files)
}

pub async fn create_worker_overlay(base: &Path, idx: usize) -> Result<WorktreeInfo, String> {
    let batch_id = uuid::Uuid::new_v4().simple().to_string();
    let short_id = &batch_id[..12.min(batch_id.len())];
    let control = base
        .join(".sen")
        .join(OVERLAY_DIR_NAME)
        .join(format!("{short_id}-{idx}"));
    let tree = control.join("tree");
    let base_owned = base.to_path_buf();
    let control_owned = control.clone();
    let tree_owned = tree.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let snapshot = || -> Result<(), String> {
            std::fs::create_dir_all(&tree_owned)
                .map_err(|e| format!("cannot create overlay tree dir: {e}"))?;
            let files = walk_snapshot_files(&base_owned)?;
            let mut manifest = OverlayManifest::default();
            for abs in files {
                let Some(rel) = rel_key(&base_owned, &abs) else {
                    continue;
                };
                let bytes = match std::fs::read(&abs) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                };
                let sha = sha256_hex(&bytes);
                let dest = tree_owned.join(&rel);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("overlay copy mkdir failed for {rel}: {e}"))?;
                }
                std::fs::write(&dest, &bytes)
                    .map_err(|e| format!("overlay copy failed for {rel}: {e}"))?;
                manifest.entries.insert(rel, sha);
            }
            let serialized = serde_json::to_vec(&manifest)
                .map_err(|e| format!("overlay manifest serialize failed: {e}"))?;
            crate::util::atomic_write(&control_owned.join(MANIFEST_FILE), &serialized)
                .map_err(|e| format!("overlay manifest write failed: {e}"))?;
            Ok(())
        };
        let result = snapshot();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&control_owned);
        }
        result
    })
    .await
    .map_err(|e| format!("overlay snapshot task failed: {e}"))??;

    tracing::info!(
        target: "workers.overlay",
        base = %base.display(),
        overlay = %tree.display(),
        "created overlay (copy-on-write snapshot) isolation for worker (no git repository detected)"
    );

    Ok(WorktreeInfo {
        path: tree,
        branch: String::new(),
        base: base.to_path_buf(),
    })
}

struct MergePlan {
    text_rels: Vec<String>,
    text_edits: Vec<crate::apply_model::EditOp>,
    deletions: Vec<(PathBuf, String, String)>,
    binary_writes: Vec<(PathBuf, Vec<u8>, Option<String>, String)>,
    conflicts: Vec<String>,
}

impl MergePlan {
    fn is_empty(&self) -> bool {
        self.text_edits.is_empty()
            && self.deletions.is_empty()
            && self.binary_writes.is_empty()
            && self.conflicts.is_empty()
    }
}

fn plan_overlay_merge(info: &WorktreeInfo) -> Result<MergePlan, String> {
    let manifest: OverlayManifest = std::fs::read(manifest_path(info))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| "overlay manifest missing or unreadable; cannot merge safely".to_string())?;

    let worker_files = walk_snapshot_files(&info.path)?;
    let mut worker_rels: BTreeSet<String> = BTreeSet::new();
    for abs in &worker_files {
        if let Some(rel) = rel_key(&info.path, abs) {
            worker_rels.insert(rel);
        }
    }
    let mut all_rels: BTreeSet<String> = worker_rels.clone();
    all_rels.extend(manifest.entries.keys().cloned());

    let mut plan = MergePlan {
        text_rels: Vec::new(),
        text_edits: Vec::new(),
        deletions: Vec::new(),
        binary_writes: Vec::new(),
        conflicts: Vec::new(),
    };

    for rel in all_rels {
        let base_sha = manifest.entries.get(&rel).cloned();
        let worker_abs = info.path.join(&rel);
        let parent_abs = info.base.join(&rel);
        let worker_present = std::fs::symlink_metadata(&worker_abs).is_ok();
        let worker_walked = worker_rels.contains(&rel);
        if worker_present && !worker_walked && base_sha.is_some() {
            plan.conflicts.push(format!(
                "{rel} (exists in overlay but excluded by snapshot filters; not merged)"
            ));
            continue;
        }
        let worker_bytes = if worker_walked {
            std::fs::read(&worker_abs).ok()
        } else {
            None
        };
        let parent_bytes = std::fs::read(&parent_abs).ok();
        let worker_sha = worker_bytes.as_deref().map(sha256_hex);
        let parent_sha = parent_bytes.as_deref().map(sha256_hex);

        match (base_sha, worker_sha) {
            (Some(base), Some(worker)) => {
                if worker == base {
                    continue;
                }
                if parent_sha.as_deref() == Some(worker.as_str()) {
                    continue;
                }
                if parent_sha.as_deref() == Some(base.as_str()) {
                    let bytes = worker_bytes.unwrap_or_default();
                    match String::from_utf8(bytes) {
                        Ok(text) => {
                            plan.text_edits.push(crate::apply_model::EditOp::CreateFile {
                                path: parent_abs.clone(),
                                contents: text,
                                overwrite: true,
                                encoding: None,
                                expected_pre_sha256: Some(base.clone()),
                            });
                            plan.text_rels.push(rel);
                        }
                        Err(raw) => {
                            plan.binary_writes.push((
                                parent_abs.clone(),
                                raw.into_bytes(),
                                Some(base.clone()),
                                rel,
                            ));
                        }
                    }
                } else {
                    plan.conflicts.push(rel);
                }
            }
            (Some(base), None) => {
                if worker_present {
                    continue;
                }
                if parent_bytes.is_none() {
                    continue;
                }
                if parent_sha.as_deref() == Some(base.as_str()) {
                    plan.deletions.push((parent_abs.clone(), base.clone(), rel));
                } else {
                    plan.conflicts.push(rel);
                }
            }
            (None, Some(worker)) => {
                if parent_bytes.is_none() {
                    let bytes = worker_bytes.unwrap_or_default();
                    match String::from_utf8(bytes) {
                        Ok(text) => {
                            plan.text_edits.push(crate::apply_model::EditOp::CreateFile {
                                path: parent_abs.clone(),
                                contents: text,
                                overwrite: false,
                                encoding: None,
                                expected_pre_sha256: None,
                            });
                            plan.text_rels.push(rel);
                        }
                        Err(raw) => {
                            plan.binary_writes.push((
                                parent_abs.clone(),
                                raw.into_bytes(),
                                None,
                                rel,
                            ));
                        }
                    }
                } else if parent_sha.as_deref() == Some(worker.as_str()) {
                    continue;
                } else {
                    plan.conflicts.push(rel);
                }
            }
            (None, None) => {}
        }
    }
    Ok(plan)
}

pub async fn merge_worker_overlay(info: &WorktreeInfo) -> Result<String, String> {
    let info_cl = info.clone();
    let plan = tokio::task::spawn_blocking(move || plan_overlay_merge(&info_cl))
        .await
        .map_err(|e| format!("overlay merge planning task failed: {e}"))??;

    let MergePlan {
        text_rels,
        text_edits,
        deletions,
        binary_writes,
        conflicts,
    } = plan;

    let mut failed: Vec<String> = Vec::new();
    let mut text_applied = 0usize;
    let mut binary_applied = 0usize;
    let mut deleted_count = 0usize;

    let mut batch_ok = true;
    if !text_edits.is_empty() {
        let applier =
            crate::apply_model::OpsApplier::locked_for_workspace(info.base.clone());
        let mut batch = crate::apply_model::EditBatch::new(
            crate::apply_model::EditOrigin::Agent {
                id: "overlay_merge".to_string(),
            },
        );
        for op in text_edits {
            batch.push(op);
        }
        match applier.apply_batch(batch).await {
            Ok(_) => {
                text_applied = text_rels.len();
            }
            Err(e) => {
                batch_ok = false;
                failed.push(format!(
                    "journaled apply failed (all text edits rolled back): {e}"
                ));
            }
        }
    }

    if batch_ok {
        for (path, bytes, expected_pre_sha, _rel) in binary_writes {
            let write_result =
                tokio::task::spawn_blocking(move || -> Result<(), String> {
                    if let Some(expected) = expected_pre_sha.as_deref() {
                        let current = std::fs::read(&path).map_err(|e| {
                            format!("{}: cannot re-read parent: {e}", path.display())
                        })?;
                        if !sha256_hex(&current).eq_ignore_ascii_case(expected) {
                            return Err(format!(
                                "{}: parent changed since snapshot; skipped binary overwrite",
                                path.display()
                            ));
                        }
                    } else if path.exists() {
                        return Err(format!(
                            "{}: parent file appeared since snapshot; skipped binary create",
                            path.display()
                        ));
                    }
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    crate::util::atomic_write(&path, &bytes)
                        .map_err(|e| format!("{}: write failed: {e}", path.display()))
                })
                .await
                .map_err(|e| format!("binary write task failed: {e}"))?;
            match write_result {
                Ok(()) => binary_applied += 1,
                Err(e) => failed.push(e),
            }
        }

        for (path, base_sha, _rel) in deletions {
            let delete_result =
                tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let current = match std::fs::read(&path) {
                        Ok(bytes) => bytes,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            return Ok(());
                        }
                        Err(e) => {
                            return Err(format!(
                                "{}: cannot re-read parent before delete: {e}",
                                path.display()
                            ));
                        }
                    };
                    if !sha256_hex(&current).eq_ignore_ascii_case(&base_sha) {
                        return Err(format!(
                            "{}: parent changed since snapshot; skipped delete",
                            path.display()
                        ));
                    }
                    std::fs::remove_file(&path)
                        .map_err(|e| format!("{}: delete failed: {e}", path.display()))
                })
                .await
                .map_err(|e| format!("delete task failed: {e}"))?;
            match delete_result {
                Ok(()) => deleted_count += 1,
                Err(e) => failed.push(e),
            }
        }
    }

    let mut summary = format!(
        "overlay merge: applied {} file(s) ({} text, {} binary), deleted {}, {} conflict(s)",
        text_applied + binary_applied,
        text_applied,
        binary_applied,
        deleted_count,
        conflicts.len()
    );
    if !conflicts.is_empty() {
        summary.push_str(&format!(
            "; conflicting files kept in overlay `{}`: {}",
            info.path.display(),
            conflicts.join(", ")
        ));
    }
    if !failed.is_empty() {
        summary.push_str(&format!("; failures: {}", failed.join("; ")));
    }
    if conflicts.is_empty() && failed.is_empty() {
        let note = remove_worker_overlay(info).await;
        if !note.is_empty() {
            summary.push_str(&note);
        }
        Ok(summary)
    } else if failed.is_empty() {
        Ok(summary)
    } else {
        Err(summary)
    }
}

pub async fn remove_worker_overlay(info: &WorktreeInfo) -> String {
    let control = control_dir(info);
    if !control.exists() {
        return String::new();
    }
    match tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&control)).await {
        Ok(Ok(())) => String::new(),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Ok(Err(e)) => format!(" (overlay cleanup failed: {e})"),
        Err(e) => format!(" (overlay cleanup task failed: {e})"),
    }
}

pub async fn salvage_worker_overlay(info: &WorktreeInfo) -> WorktreeSalvage {
    let info_cl = info.clone();
    let plan = tokio::task::spawn_blocking(move || plan_overlay_merge(&info_cl)).await;
    if let Ok(Ok(plan)) = plan {
        if plan.is_empty() {
            let note = remove_worker_overlay(info).await;
            return WorktreeSalvage {
                note: format!("no_changes: overlay had no unmerged changes and was removed{note}"),
                retained: false,
            };
        }
    }
    WorktreeSalvage {
        note: format!(
            "overlay retained at `{}` for manual recovery (no git branch available without a repository)",
            info.path.display()
        ),
        retained: true,
    }
}

pub async fn overlay_change_report(info: &WorktreeInfo) -> String {
    let info_cl = info.clone();
    let outcome = tokio::task::spawn_blocking(move || plan_overlay_merge(&info_cl)).await;
    match outcome {
        Ok(Ok(plan)) => {
            if plan.is_empty() {
                "(no file changes)".to_string()
            } else {
                let mut lines: Vec<String> = Vec::new();
                for rel in &plan.text_rels {
                    lines.push(format!("M {rel}"));
                }
                for (_, _, _, rel) in &plan.binary_writes {
                    lines.push(format!("M {rel} (binary)"));
                }
                for (_, _, rel) in &plan.deletions {
                    lines.push(format!("D {rel}"));
                }
                for rel in &plan.conflicts {
                    lines.push(format!("C {rel} (conflicts with parent changes)"));
                }
                lines.join("\n")
            }
        }
        Ok(Err(e)) => format!("(overlay change report unavailable: {e})"),
        Err(e) => format!("(overlay change report task failed: {e})"),
    }
}
