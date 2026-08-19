// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::apply_model::{
    ApplyBatchError, ApplyError, ApplyOptions, EditBatch, EditOp, EditOrigin, OpsApplier,
};
use crate::observability::session_write_mode_metrics;

#[derive(Debug, Clone)]
struct FileBackup {

    original: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
enum StagedChange {
    Diff {
        path: PathBuf,
        diff: String,
    },
    FullContent {
        path: PathBuf,
        contents: String,
        encoding: Option<String>,
        pre_sha256: Option<String>,
    },
}

impl StagedChange {
    fn path(&self) -> &PathBuf {
        match self {
            StagedChange::Diff { path, .. } | StagedChange::FullContent { path, .. } => path,
        }
    }
}

#[derive(Debug)]
pub struct DiffSession {
    root: PathBuf,
    allowed_roots: Vec<PathBuf>,
    staged: Vec<StagedChange>,
    backups: BTreeMap<PathBuf, FileBackup>,
    applied: bool,
    apply_opts: ApplyOptions,

    ops_applier: Option<Arc<OpsApplier>>,

    last_batch_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApplyReport {
    pub files_touched: Vec<PathBuf>,
    pub total_hunks_exact: usize,
    pub total_hunks_fuzzy: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum DiffSessionError {
    #[error("path escapes workspace root: {0}")]
    PathEscape(PathBuf),
    #[error("session already applied; call rollback() or build a new one")]
    AlreadyApplied,
    #[error("session has not been applied yet")]
    NotApplied,
    #[error("read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("apply {path}: {source}")]
    Apply {
        path: PathBuf,
        #[source]
        source: ApplyError,
    },

    #[error("restore failed for {path}: {source}")]
    RestorePartial {
        path: PathBuf,
        written_paths: Vec<PathBuf>,
        #[source]
        source: std::io::Error,
    },

    #[error("atomic restore failed: unable to stage {path}: {source}")]
    RestoreStageFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("ops_applier failure: {reason}")]
    OpsApplier { reason: String },
}

impl DiffSession {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            allowed_roots: Vec::new(),
            staged: Vec::new(),
            backups: BTreeMap::new(),
            applied: false,
            apply_opts: ApplyOptions {
                max_fuzz: 3,
                dry_run: false,
                validate: true,
                path: None,
            },
            ops_applier: None,
            last_batch_id: None,
        }
    }

    #[must_use]
    pub fn with_allowed_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.allowed_roots = roots;
        self
    }

    #[must_use]
    pub fn with_apply_options(mut self, opts: ApplyOptions) -> Self {
        self.apply_opts = opts;
        self
    }

    #[must_use]
    pub fn with_ops_applier(mut self, ops_applier: Arc<OpsApplier>) -> Self {
        self.ops_applier = Some(ops_applier);
        self
    }

    fn ops_applier(&self) -> Arc<OpsApplier> {
        if let Some(o) = self.ops_applier.clone() {
            return o;
        }
        Arc::new(OpsApplier::locked_for_workspace(self.root.clone()))
    }

    pub fn stage(
        &mut self,
        path: impl AsRef<Path>,
        diff: impl Into<String>,
    ) -> Result<(), DiffSessionError> {
        if self.applied {
            return Err(DiffSessionError::AlreadyApplied);
        }
        let abs = resolve_inside(&self.root, &self.allowed_roots, path.as_ref())?;
        self.staged.push(StagedChange::Diff {
            path: abs,
            diff: diff.into(),
        });
        Ok(())
    }

    pub fn stage_full_content(
        &mut self,
        path: impl AsRef<Path>,
        contents: impl Into<String>,
        encoding: Option<String>,
        pre_sha256: Option<String>,
    ) -> Result<(), DiffSessionError> {
        if self.applied {
            return Err(DiffSessionError::AlreadyApplied);
        }
        let abs = resolve_inside(&self.root, &self.allowed_roots, path.as_ref())?;
        self.staged.push(StagedChange::FullContent {
            path: abs,
            contents: contents.into(),
            encoding,
            pre_sha256,
        });
        Ok(())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.staged.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.staged.is_empty()
    }

    #[must_use]
    pub fn staged_paths(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.staged.iter().map(|s| s.path().clone()).collect();
        out.sort();
        out.dedup();
        out
    }

    pub async fn apply_all(&mut self) -> Result<ApplyReport, DiffSessionError> {
        if self.applied {
            return Err(DiffSessionError::AlreadyApplied);
        }

        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let to_backup: Vec<PathBuf> = self
            .staged
            .iter()
            .map(|s| s.path().clone())
            .filter(|p| !self.backups.contains_key(p) && seen.insert(p.clone()))
            .collect();
        if !to_backup.is_empty() {
            let reads = tokio::task::spawn_blocking(move || {
                to_backup
                    .into_iter()
                    .map(|p| {
                        let original = match std::fs::read(&p) {
                            Ok(bytes) => Ok(Some(bytes)),
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                            Err(e) => Err((p.clone(), e)),
                        };
                        (p, original)
                    })
                    .collect::<Vec<(
                        PathBuf,
                        Result<Option<Vec<u8>>, (PathBuf, std::io::Error)>,
                    )>>()
            })
            .await
            .map_err(|e| DiffSessionError::OpsApplier {
                reason: format!("backup read task failed: {e}"),
            })?;
            for (path, original) in reads {
                let original = original.map_err(|(path, source)| DiffSessionError::Io {
                    path,
                    source,
                })?;
                self.backups
                    .entry(path)
                    .or_insert(FileBackup { original });
            }
        }

        let mut batch = EditBatch::new(EditOrigin::DiffSession).with_atomic(true);
        let fuzz = self.apply_opts.max_fuzz.min(u8::MAX as usize) as u8;
        for staged in &self.staged {
            match staged {
                StagedChange::Diff { path, diff } => {
                    batch.push(EditOp::ApplyHunk {
                        path: path.clone(),
                        diff: diff.clone(),
                        fuzz,
                        scope_anchor: None,
                    });
                }
                StagedChange::FullContent {
                    path,
                    contents,
                    encoding,
                    pre_sha256,
                } => {
                    batch.push(EditOp::CreateFile {
                        path: path.clone(),
                        contents: contents.clone(),
                        overwrite: true,
                        encoding: encoding.clone(),
                        expected_pre_sha256: pre_sha256.clone(),
                    });
                }
            }
        }
        let touched_paths = batch
            .ops
            .iter()
            .map(|op| op.primary_path().to_path_buf())
            .collect::<Vec<_>>();
        let batch_id = batch.batch_id.clone();
        let applier = self.ops_applier();

        let result = applier.apply_batch(batch).await;
        match result {
            Ok(_) => {
                self.applied = true;
                self.last_batch_id = Some(batch_id);
                session_write_mode_metrics::incr_diff_session_applied();
                Ok(ApplyReport {
                    files_touched: touched_paths,

                    total_hunks_exact: 0,
                    total_hunks_fuzzy: 0,
                })
            }
            Err(e) => {
                if matches!(e, ApplyBatchError::RollbackFailed { .. }) {
                    let root = self.root.clone();
                    let backups = self.backups.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        recover_with_atomic_fallback(&root, &backups)
                    })
                    .await;
                }
                Err(DiffSessionError::OpsApplier {
                    reason: e.to_string(),
                })
            }
        }
    }

    pub async fn rollback(&mut self) -> Result<(), DiffSessionError> {
        if !self.applied {
            return Err(DiffSessionError::NotApplied);
        }

        if let Some(batch_id) = self.last_batch_id.clone() {
            let applier = self.ops_applier();
            let result = applier.rollback(&batch_id).await;
            if result.is_ok() {
                self.applied = false;
                self.last_batch_id = None;
                session_write_mode_metrics::incr_diff_session_rollback();
                return Ok(());
            }
        }

        {
            let root = self.root.clone();
            let backups = self.backups.clone();
            tokio::task::spawn_blocking(move || restore_backups_atomic(&root, &backups))
                .await
                .map_err(|e| DiffSessionError::OpsApplier {
                    reason: format!("restore task failed: {e}"),
                })??;
        }
        self.applied = false;
        self.last_batch_id = None;
        session_write_mode_metrics::incr_diff_session_rollback();
        Ok(())
    }

}

fn recover_with_atomic_fallback(root: &Path, backups: &BTreeMap<PathBuf, FileBackup>) {
    match restore_backups_atomic(root, backups) {
        Ok(()) => {}
        Err(e) => {
            tracing::error!(
                target: "diff_session",
                error = %e,
                "atomic restore failed during apply_all recovery; falling back to best-effort restore",
            );
            restore_backups_best_effort(backups);
        }
    }
}

fn restore_backups_best_effort(backups: &BTreeMap<PathBuf, FileBackup>) {
    for (path, backup) in backups {
        match &backup.original {
            Some(bytes) => {
                let _ = std::fs::write(path, bytes);
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn restore_backups_atomic(
    root: &Path,
    backups: &BTreeMap<PathBuf, FileBackup>,
) -> Result<(), DiffSessionError> {
    use std::fs;

    if backups.is_empty() {
        return Ok(());
    }

    let staging_root = root
        .join(".sen")
        .join("diff_session")
        .join(format!("restore-{}", uuid::Uuid::new_v4()));

        fs::create_dir_all(&staging_root).map_err(|source| {
            DiffSessionError::RestoreStageFailed {
                path: staging_root.clone(),
                source,
            }
        })?;

        struct Staged {
            staged: PathBuf,
            original: PathBuf,
            delete_original: bool,
        }
        let mut stage_plan: Vec<Staged> = Vec::with_capacity(backups.len());

        for (path, backup) in backups {
            match &backup.original {
                Some(bytes) => {
                    let rel = path.strip_prefix(root).unwrap_or(path);

                    let rel_clean: PathBuf = rel
                        .components()
                        .filter(|c| {
                            !matches!(
                                c,
                                std::path::Component::RootDir | std::path::Component::Prefix(_)
                            )
                        })
                        .collect();
                    let staged = staging_root.join(&rel_clean);
                    if let Some(parent) = staged.parent() {
                        fs::create_dir_all(parent).map_err(|source| {
                            DiffSessionError::RestoreStageFailed {
                                path: parent.to_path_buf(),
                                source,
                            }
                        })?;
                    }
                    fs::write(&staged, bytes).map_err(|source| {
                        DiffSessionError::RestoreStageFailed {
                            path: staged.clone(),
                            source,
                        }
                    })?;
                    stage_plan.push(Staged {
                        staged,
                        original: path.clone(),
                        delete_original: false,
                    });
                }
                None => {
                    stage_plan.push(Staged {
                        staged: PathBuf::new(),
                        original: path.clone(),
                        delete_original: true,
                    });
                }
            }
        }

        let mut applied: Vec<PathBuf> = Vec::with_capacity(stage_plan.len());
        for Staged {
            staged,
            original,
            delete_original,
        } in &stage_plan
        {
            let r = if *delete_original {
                match fs::remove_file(original) {
                    Ok(_) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e),
                }
            } else {

                match fs::rename(staged, original) {
                    Ok(()) => Ok(()),
                    Err(_) => match fs::copy(staged, original) {
                        Ok(_) => {
                            let _ = fs::remove_file(staged);
                            Ok(())
                        }
                        Err(e) => Err(e),
                    },
                }
            };
            match r {
                Ok(()) => applied.push(original.clone()),
                Err(source) => {
                    let _ = fs::remove_dir_all(&staging_root);
                    return Err(DiffSessionError::RestorePartial {
                        path: original.clone(),
                        written_paths: applied,
                        source,
                    });
                }
            }
        }

        let _ = fs::remove_dir_all(&staging_root);
        Ok(())
}

fn resolve_inside(
    root: &Path,
    allowed_roots: &[PathBuf],
    path: &Path,
) -> Result<PathBuf, DiffSessionError> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut normal = PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::ParentDir => {
                if !normal.pop() {
                    return Err(DiffSessionError::PathEscape(joined));
                }
            }
            std::path::Component::CurDir => {}
            other => normal.push(other.as_os_str()),
        }
    }
    if crate::util::path_is_within(&normal, root) {
        return Ok(normal);
    }
    for extra in allowed_roots {
        if crate::util::path_is_within(&normal, extra) {
            return Ok(normal);
        }
    }
    Err(DiffSessionError::PathEscape(normal))
}
