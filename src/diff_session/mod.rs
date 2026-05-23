// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::apply_model::{
    ApplyError, ApplyOptions, EditBatch, EditOp, EditOrigin, OpsApplier,
};
use crate::observability::session_write_mode_metrics;

#[derive(Debug, Clone)]
struct FileBackup {

    original: Option<String>,
}

#[derive(Debug, Clone)]
struct StagedDiff {
    path: PathBuf,
    diff: String,
}

#[derive(Debug)]
pub struct DiffSession {
    root: PathBuf,
    staged: Vec<StagedDiff>,
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
            staged: Vec::new(),
            backups: BTreeMap::new(),
            applied: false,
            apply_opts: ApplyOptions {
                max_fuzz: 3,
                dry_run: false,
                validate: true,
            },
            ops_applier: None,
            last_batch_id: None,
        }
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
        Arc::new(OpsApplier::default_for_workspace(self.root.clone()))
    }

    pub fn stage(
        &mut self,
        path: impl AsRef<Path>,
        diff: impl Into<String>,
    ) -> Result<(), DiffSessionError> {
        if self.applied {
            return Err(DiffSessionError::AlreadyApplied);
        }
        let abs = resolve_inside(&self.root, path.as_ref())?;
        self.staged.push(StagedDiff {
            path: abs,
            diff: diff.into(),
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
        let mut out: Vec<PathBuf> = self.staged.iter().map(|s| s.path.clone()).collect();
        out.sort();
        out.dedup();
        out
    }

    pub async fn apply_all(&mut self) -> Result<ApplyReport, DiffSessionError> {
        if self.applied {
            return Err(DiffSessionError::AlreadyApplied);
        }

        for staged in &self.staged {
            self.backups
                .entry(staged.path.clone())
                .or_insert_with(|| FileBackup {
                    original: std::fs::read_to_string(&staged.path).ok(),
                });
        }

        let mut batch = EditBatch::new(EditOrigin::DiffSession).with_atomic(true);
        let fuzz = self.apply_opts.max_fuzz.min(u8::MAX as usize) as u8;
        for staged in &self.staged {
            batch.push(EditOp::ApplyHunk {
                path: staged.path.clone(),
                diff: staged.diff.clone(),
                fuzz,
                scope_anchor: None,
            });
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

                self.recover_with_atomic_fallback();
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

        self.restore_backups_atomic()?;
        self.applied = false;
        self.last_batch_id = None;
        session_write_mode_metrics::incr_diff_session_rollback();
        Ok(())
    }

    fn recover_with_atomic_fallback(&self) {
        match self.restore_backups_atomic() {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(
                    target: "diff_session",
                    error = %e,
                    "atomic restore failed during apply_all recovery; falling back to best-effort restore",
                );
                self.restore_backups_best_effort();
            }
        }
    }

    fn restore_backups_best_effort(&self) {
        for (path, backup) in &self.backups {
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

    fn restore_backups_strict(&self) -> Result<(), DiffSessionError> {
        let mut written: Vec<PathBuf> = Vec::new();
        for (path, backup) in &self.backups {
            let r = match &backup.original {
                Some(bytes) => std::fs::write(path, bytes),
                None => match std::fs::remove_file(path) {
                    Ok(_) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e),
                },
            };
            match r {
                Ok(()) => written.push(path.clone()),
                Err(source) => {
                    return Err(DiffSessionError::RestorePartial {
                        path: path.clone(),
                        written_paths: written,
                        source,
                    });
                }
            }
        }
        Ok(())
    }

    fn restore_backups_atomic(&self) -> Result<(), DiffSessionError> {
        use std::fs;

        if self.backups.is_empty() {
            return Ok(());
        }

        let staging_root = self
            .root
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
        let mut stage_plan: Vec<Staged> = Vec::with_capacity(self.backups.len());

        for (path, backup) in &self.backups {
            match &backup.original {
                Some(bytes) => {
                    let rel = path.strip_prefix(&self.root).unwrap_or(path);

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
}


fn resolve_inside(root: &Path, path: &Path) -> Result<PathBuf, DiffSessionError> {
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
    if !normal.starts_with(root) {
        return Err(DiffSessionError::PathEscape(normal));
    }
    Ok(normal)
}
