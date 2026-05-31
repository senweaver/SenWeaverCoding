// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::edit_op::{EditBatch, EditOp, NotebookCellOp, PreconditionError};
use super::heuristic::apply_unified_diff;
use super::traits::{ApplyError, ApplyOptions};
use super::validator::validate_bytes;

pub trait LockGuard: Send + Sync {}

#[derive(Debug, Clone)]
pub struct RegionLockRequest {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub exclusive: bool,
}

#[async_trait]
pub trait LockProvider: Send + Sync {
    async fn acquire_for_paths(
        &self,
        paths: &[PathBuf],
        holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError>;

    async fn acquire_for_regions(
        &self,
        regions: &[RegionLockRequest],
        holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError> {
        let mut seen: std::collections::BTreeSet<PathBuf> =
            std::collections::BTreeSet::new();
        for r in regions {
            seen.insert(r.path.clone());
        }
        let paths: Vec<PathBuf> = seen.into_iter().collect();
        self.acquire_for_paths(&paths, holder).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LockProviderError {
    #[error("lock acquire failed: {0}")]
    Acquire(String),
}

pub struct NoopLockGuard;
impl LockGuard for NoopLockGuard {}

#[derive(Debug, Default, Clone)]
pub struct NoopLockProvider;

#[async_trait]
impl LockProvider for NoopLockProvider {
    async fn acquire_for_paths(
        &self,
        _paths: &[PathBuf],
        _holder: &str,
    ) -> Result<Box<dyn LockGuard>, LockProviderError> {
        Ok(Box::new(NoopLockGuard))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BatchValidatorError {
    #[error("validator rejected batch: {0}")]
    Rejected(String),
    #[error("validator infrastructure failure: {0}")]
    Infrastructure(String),
}

#[async_trait]
pub trait BatchValidator: Send + Sync {
    async fn validate(
        &self,
        batch: &EditBatch,
        preview: &BatchPreview,
    ) -> Result<(), BatchValidatorError>;
}

#[derive(Debug, Default, Clone)]
pub struct NoopBatchValidator;

#[async_trait]
impl BatchValidator for NoopBatchValidator {
    async fn validate(
        &self,
        _batch: &EditBatch,
        _preview: &BatchPreview,
    ) -> Result<(), BatchValidatorError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpOutcome {
    pub op_index: usize,
    pub touched_path: PathBuf,
    pub bytes_before: Option<usize>,
    pub bytes_after: Option<usize>,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOutcome {
    pub batch_id: String,
    pub touched_paths: Vec<PathBuf>,
    pub per_op: Vec<OpOutcome>,
    pub journal_path: Option<PathBuf>,

    pub degraded: bool,

    pub journal_persisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedDiffPreview {
    pub op_index: usize,
    pub path: PathBuf,
    pub unified_diff: String,
    pub before_bytes: Option<usize>,
    pub after_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPreview {
    pub batch_id: String,
    pub diffs: Vec<UnifiedDiffPreview>,
    pub created: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    pub renamed: Vec<(PathBuf, PathBuf)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyBatchError {
    #[error("precondition failed: {0}")]
    Precondition(#[from] PreconditionError),
    #[error("lock acquire failed: {0}")]
    Lock(#[from] LockProviderError),
    #[error("validator rejected batch: {0}")]
    Validator(#[from] BatchValidatorError),
    #[error("apply failed for op #{op_index} ({path}): {source}")]
    Apply {
        op_index: usize,
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("apply_unified_diff failed for op #{op_index} ({path}): {source}")]
    Hunk {
        op_index: usize,
        path: PathBuf,
        #[source]
        source: ApplyError,
    },
    #[error("io error for op #{op_index} ({path}): {source}")]
    Io {
        op_index: usize,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("rollback failed after primary error '{primary}': {rollback}")]
    RollbackFailed {
        primary: String,
        rollback: String,
    },
    #[error("journal write failed: {0}")]
    Journal(#[source] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum RollbackError {
    #[error("journal not found for batch {0}")]
    JournalMissing(String),
    #[error("journal parse failed: {0}")]
    Parse(String),
    #[error("rollback io error for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalHeader {
    batch_id: String,
    correlation_id: Option<String>,
    origin: String,
    atomic: bool,
    started_at: DateTime<Utc>,
    workspace_root: PathBuf,
    #[serde(default)]
    status: JournalStatus,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalStatus {
    #[default]
    Pending,
    Committed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalRecord {
    op_index: usize,
    op: EditOp,
    pre_image: Option<PreImage>,
    post_image_sha256: Option<String>,
    ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PreImage {

    path: PathBuf,

    bytes: Option<Vec<u8>>,

    rename_target_bytes: Option<Vec<u8>>,
    sha256: Option<String>,
    mtime_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalLineKind {
    Header,
    Record,
    Footer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalLine {
    kind: JournalLineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    header: Option<JournalHeader>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record: Option<JournalRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    footer: Option<JournalFooter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalFooter {
    status: JournalStatus,
    finished_at: DateTime<Utc>,
    degraded: bool,
}

pub struct OpsApplier {
    workspace_root: Arc<RwLock<PathBuf>>,
    lock_provider: Arc<dyn LockProvider>,
    validator: Arc<dyn BatchValidator>,
    apply_opts: ApplyOptions,
    journal_retention: usize,

    symbol_graph_writer: Option<Arc<crate::code_intel::symbol_graph::incremental::SymbolGraphWriter>>,

    lsp_notify: Option<Arc<dyn LspNotifier>>,

    edit_history: Option<Arc<crate::tools::edit_history::EditHistory>>,
}

#[async_trait::async_trait]
pub trait LspNotifier: Send + Sync {
    async fn notify_changed(&self, path: &Path, contents: &str) -> anyhow::Result<()>;
}

impl OpsApplier {
    #[inline]
    fn workspace_snapshot(&self) -> PathBuf {
        self.workspace_root.read().clone()
    }

    #[inline]
    fn journal_dir_for_workspace(ws: &Path) -> PathBuf {
        ws.join(".sen").join("edit_journal")
    }

    #[inline]
    fn journal_dir_snapshot(&self) -> PathBuf {
        Self::journal_dir_for_workspace(&self.workspace_snapshot())
    }

    #[must_use]
    pub fn default_for_workspace(workspace_root: impl Into<PathBuf>) -> Self {
        let raw = workspace_root.into();
        let canon = std::fs::canonicalize(&raw).unwrap_or(raw);
        Self::default_for_shared_workspace(Arc::new(RwLock::new(canon)))
    }

    #[must_use]
    pub fn default_for_shared_workspace(workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        Self {
            workspace_root,
            lock_provider: Arc::new(NoopLockProvider),
            validator: Arc::new(NoopBatchValidator),
            apply_opts: ApplyOptions::default(),
            journal_retention: 64,
            symbol_graph_writer: None,
            lsp_notify: None,
            edit_history: None,
        }
    }

    #[must_use]
    pub fn with_edit_history(
        mut self,
        history: Arc<crate::tools::edit_history::EditHistory>,
    ) -> Self {
        self.edit_history = Some(history);
        self
    }

    #[must_use]
    pub fn with_symbol_graph_writer(
        mut self,
        writer: Arc<crate::code_intel::symbol_graph::incremental::SymbolGraphWriter>,
    ) -> Self {
        self.symbol_graph_writer = Some(writer);
        self
    }

    #[must_use]
    pub fn with_lsp_notifier(mut self, notifier: Arc<dyn LspNotifier>) -> Self {
        self.lsp_notify = Some(notifier);
        self
    }

    #[must_use]
    pub fn with_lock_provider(mut self, provider: Arc<dyn LockProvider>) -> Self {
        self.lock_provider = provider;
        self
    }

    #[must_use]
    pub fn with_validator(mut self, validator: Arc<dyn BatchValidator>) -> Self {
        self.validator = validator;
        self
    }

    #[must_use]
    pub fn with_apply_options(mut self, opts: ApplyOptions) -> Self {
        self.apply_opts = opts;
        self
    }

    #[must_use]
    pub fn with_journal_retention(mut self, retention: usize) -> Self {
        self.journal_retention = retention;
        self
    }

    #[must_use]
    pub fn workspace_root(&self) -> PathBuf {
        self.workspace_snapshot()
    }

    pub async fn apply_batch(
        &self,
        batch: EditBatch,
    ) -> Result<BatchOutcome, ApplyBatchError> {
        let ws = self.workspace_snapshot();
        for op in &batch.ops {
            op.validate_preconditions(&ws)?;
        }

        let unique_paths = unique_touched_paths(&batch);
        let region_requests = region_requests_for_batch(&batch);
        let _guard = self
            .lock_provider
            .acquire_for_regions(&region_requests, batch.origin.tag())
            .await?;

        let pre_images: Arc<BTreeMap<PathBuf, PreImage>> = {
            let batch_clone = batch.clone();
            match tokio::task::spawn_blocking(move || capture_pre_images(&batch_clone)).await {
                Ok(res) => Arc::new(res?),
                Err(e) => {
                    return Err(ApplyBatchError::Io {
                        op_index: 0,
                        path: PathBuf::new(),
                        source: std::io::Error::other(format!(
                            "capture_pre_images join error: {e}"
                        )),
                    });
                }
            }
        };

        let (journal_path, journal_persisted) = self
            .write_journal_pending(&batch, Arc::clone(&pre_images))
            .await?;

        let preview = self.build_preview(&batch).await?;

        let mut per_op: Vec<OpOutcome> = Vec::with_capacity(batch.ops.len());
        let mut degraded = false;
        let mut applied_paths: Vec<PathBuf> = Vec::new();

        for (idx, op) in batch.ops.iter().enumerate() {
            let touched = op.primary_path().to_path_buf();
            match self.apply_one(idx, op).await {
                Ok((before, after)) => {
                    per_op.push(OpOutcome {
                        op_index: idx,
                        touched_path: touched.clone(),
                        bytes_before: before,
                        bytes_after: after,
                        success: true,
                        error: None,
                    });
                    applied_paths.push(touched);
                }
                Err(err) => {
                    let msg = err.to_string();
                    per_op.push(OpOutcome {
                        op_index: idx,
                        touched_path: touched,
                        bytes_before: None,
                        bytes_after: None,
                        success: false,
                        error: Some(msg.clone()),
                    });
                    if batch.atomic {

                        if let Err(rb) = restore_pre_images(&pre_images) {
                            self.append_footer(
                                journal_path.as_deref(),
                                JournalStatus::RolledBack,
                                true,
                            );
                            return Err(ApplyBatchError::RollbackFailed {
                                primary: msg,
                                rollback: rb.to_string(),
                            });
                        }
                        self.append_footer(
                            journal_path.as_deref(),
                            JournalStatus::RolledBack,
                            false,
                        );
                        return Err(err);
                    }
                    degraded = true;
                }
            }
        }

        if let Err(verr) = self.validator.validate(&batch, &preview).await {
            if batch.atomic {
                if let Err(rb) = restore_pre_images(&pre_images) {
                    self.append_footer(
                        journal_path.as_deref(),
                        JournalStatus::RolledBack,
                        true,
                    );
                    return Err(ApplyBatchError::RollbackFailed {
                        primary: verr.to_string(),
                        rollback: rb.to_string(),
                    });
                }
                self.append_footer(
                    journal_path.as_deref(),
                    JournalStatus::RolledBack,
                    false,
                );
                return Err(verr.into());
            }
            degraded = true;
        }

        if self.apply_opts.validate {
            for path in &applied_paths {
                let path_for_read = path.clone();
                let read =
                    tokio::task::spawn_blocking(move || std::fs::read_to_string(&path_for_read))
                        .await;
                if let Ok(Ok(text)) = read {
                    let report = validate_bytes(&text);
                    if !report.is_ok() {
                        if batch.atomic {
                            let _ = restore_pre_images(&pre_images);
                            self.append_footer(
                                journal_path.as_deref(),
                                JournalStatus::RolledBack,
                                false,
                            );
                            return Err(ApplyBatchError::Validator(
                                BatchValidatorError::Rejected(format!(
                                    "{}: {}",
                                    path.display(),
                                    report
                                        .issues
                                        .iter()
                                        .map(|i| i.message.clone())
                                        .collect::<Vec<_>>()
                                        .join("; ")
                                )),
                            ));
                        }
                        degraded = true;
                    }
                }
            }
        }

        self.append_footer(journal_path.as_deref(), JournalStatus::Committed, degraded);
        self.rotate_journals();

        if let Some(history) = self.edit_history.as_ref() {
            history.stamp_latest_with_batch(unique_paths.iter(), &batch.batch_id);
        }

        if let Some(writer) = self.symbol_graph_writer.as_ref() {
            let changed: Vec<PathBuf> = unique_paths.to_vec();
            writer.on_files_changed(&changed);
        }

        if let Some(notifier) = self.lsp_notify.as_ref() {
            let applied_dedup: std::collections::HashSet<PathBuf> =
                applied_paths.iter().cloned().collect();
            for p in applied_dedup {
                let p_for_read = p.clone();
                let read = tokio::task::spawn_blocking(move || std::fs::read_to_string(&p_for_read))
                    .await;
                if let Ok(Ok(contents)) = read {
                    if notifier.notify_changed(&p, &contents).await.is_ok() {
                        crate::observability::code_intel_metrics::incr_lsp_did_change_sent();
                    }
                }
            }
        }

        Ok(BatchOutcome {
            batch_id: batch.batch_id,
            touched_paths: unique_paths,
            per_op,
            journal_path,
            degraded,
            journal_persisted,
        })
    }

    pub async fn apply_unified_diff_with_fast_path(
        &self,
        path: std::path::PathBuf,
        raw_diff: &str,
        options: &super::traits::ApplyOptions,
        refiner: Option<&super::fast_apply::FastApplyRefiner>,
        hint: Option<&str>,
        origin: super::edit_op::EditOrigin,
    ) -> Result<(BatchOutcome, super::fast_apply::FastPathTier), ApplyBatchError> {
        let source = {
            let path_for_read = path.clone();
            tokio::task::spawn_blocking(move || std::fs::read_to_string(&path_for_read))
                .await
                .map_err(|e| ApplyBatchError::Io {
                    op_index: 0,
                    path: path.clone(),
                    source: std::io::Error::other(format!("read join error: {e}")),
                })?
                .map_err(|source| ApplyBatchError::Io {
                    op_index: 0,
                    path: path.clone(),
                    source,
                })?
        };
        let (outcome, _final_diff, tier) =
            super::fast_apply::apply_unified_diff_with_fast_path(
                &source, raw_diff, options, refiner, hint,
            )
            .await
            .map_err(|e| ApplyBatchError::Hunk {
                op_index: 0,
                path: path.clone(),
                source: e,
            })?;
        let new_text = outcome.applied;
        let len_before = source.len();
        let op = super::edit_op::EditOp::Replace {
            path: path.clone(),
            byte_range: 0..len_before,
            old_text: source,
            new_text,
            anchor: None,
        };
        let batch = super::edit_op::EditBatch::new(origin)
            .with_op(op)
            .with_atomic(true);
        let result = self.apply_batch(batch).await?;
        Ok((result, tier))
    }

    pub async fn dry_run(
        &self,
        batch: &EditBatch,
    ) -> Result<BatchPreview, ApplyBatchError> {
        let ws = self.workspace_snapshot();
        for op in &batch.ops {
            op.validate_preconditions(&ws)?;
        }
        self.build_preview(batch).await
    }

    pub async fn rollback(&self, batch_id: &str) -> Result<(), RollbackError> {
        let journal_dir = self.journal_dir_snapshot();
        let batch_id = batch_id.to_string();
        let join = tokio::task::spawn_blocking(move || {
            let path = journal_dir.join(format!("{batch_id}.jsonl"));
            if !path.exists() {
                return Err(RollbackError::JournalMissing(batch_id));
            }
            let raw = std::fs::read_to_string(&path).map_err(|source| RollbackError::Io {
                path: path.clone(),
                source,
            })?;
            let mut records: Vec<JournalRecord> = Vec::new();
            for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                let parsed: JournalLine = serde_json::from_str(line)
                    .map_err(|e| RollbackError::Parse(e.to_string()))?;
                if let JournalLineKind::Record = parsed.kind {
                    if let Some(rec) = parsed.record {
                        records.push(rec);
                    }
                }
            }

            for record in records.into_iter().rev() {
                if let Some(pre) = &record.pre_image {
                    restore_one(pre).map_err(|source| RollbackError::Io {
                        path: pre.path.clone(),
                        source,
                    })?;
                }
                if let EditOp::RenameFile { from, .. } = &record.op {
                    let _ = std::fs::remove_file(record.op.primary_path());
                    let _ = from;
                }
            }

            append_footer_to_path(&path, JournalStatus::RolledBack, false);
            Ok(())
        })
        .await;
        match join {
            Ok(res) => res,
            Err(e) => Err(RollbackError::Io {
                path: PathBuf::new(),
                source: std::io::Error::other(format!("rollback join error: {e}")),
            }),
        }
    }

    async fn apply_one(
        &self,
        op_index: usize,
        op: &EditOp,
    ) -> Result<(Option<usize>, Option<usize>), ApplyBatchError> {
        let op = op.clone();
        let apply_opts = self.apply_opts.clone();
        let join = tokio::task::spawn_blocking(move || {
            let op = &op;
            let apply_opts = &apply_opts;
            match op {
            EditOp::Replace {
                path,
                byte_range,
                new_text,
                ..
            } => {
                let bytes = std::fs::read(path).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                let before = bytes.len();
                validate_byte_range_for_apply(op_index, path, byte_range, before)?;
                let mut out = Vec::with_capacity(
                    before - (byte_range.end - byte_range.start) + new_text.len(),
                );
                out.extend_from_slice(&bytes[..byte_range.start]);
                out.extend_from_slice(new_text.as_bytes());
                out.extend_from_slice(&bytes[byte_range.end..]);
                std::fs::write(path, &out).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                Ok((Some(before), Some(out.len())))
            }
            EditOp::Insert { path, at_byte, text, .. } => {
                let bytes = std::fs::read(path).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                let before = bytes.len();
                validate_at_byte_for_apply(op_index, path, *at_byte, before)?;
                let mut out = Vec::with_capacity(bytes.len() + text.len());
                out.extend_from_slice(&bytes[..*at_byte]);
                out.extend_from_slice(text.as_bytes());
                out.extend_from_slice(&bytes[*at_byte..]);
                std::fs::write(path, &out).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                Ok((Some(before), Some(out.len())))
            }
            EditOp::Delete {
                path, byte_range, ..
            } => {
                let bytes = std::fs::read(path).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                let before = bytes.len();
                validate_byte_range_for_apply(op_index, path, byte_range, before)?;
                let mut out =
                    Vec::with_capacity(before - (byte_range.end - byte_range.start));
                out.extend_from_slice(&bytes[..byte_range.start]);
                out.extend_from_slice(&bytes[byte_range.end..]);
                std::fs::write(path, &out).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                Ok((Some(before), Some(out.len())))
            }
            EditOp::CreateFile {
                path,
                contents,
                overwrite: _,
            } => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| ApplyBatchError::Io {
                        op_index,
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                std::fs::write(path, contents.as_bytes()).map_err(|source| {
                    ApplyBatchError::Io {
                        op_index,
                        path: path.clone(),
                        source,
                    }
                })?;
                Ok((None, Some(contents.len())))
            }
            EditOp::DeleteFile { path, missing_ok } => match std::fs::remove_file(path) {
                Ok(()) => Ok((None, None)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && *missing_ok => {
                    Ok((None, None))
                }
                Err(source) => Err(ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                }),
            },
            EditOp::RenameFile {
                from,
                to,
                overwrite: _,
            } => {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| ApplyBatchError::Io {
                        op_index,
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                match std::fs::rename(from, to) {
                    Ok(()) => Ok((None, None)),
                    Err(_) => {

                        std::fs::copy(from, to).map_err(|source| ApplyBatchError::Io {
                            op_index,
                            path: to.clone(),
                            source,
                        })?;
                        std::fs::remove_file(from).map_err(|source| ApplyBatchError::Io {
                            op_index,
                            path: from.clone(),
                            source,
                        })?;
                        Ok((None, None))
                    }
                }
            }
            EditOp::ApplyHunk {
                path,
                diff,
                fuzz,
                scope_anchor,
            } => {
                let source = std::fs::read_to_string(path).map_err(|source| {
                    ApplyBatchError::Io {
                        op_index,
                        path: path.clone(),
                        source,
                    }
                })?;
                let before = source.len();
                let mut opts = apply_opts.clone();
                opts.max_fuzz = *fuzz as usize;
                opts.dry_run = false;

                let outcome = if let Some(anchor) = scope_anchor.as_ref() {
                    let named = build_named_scopes_for(path, anchor);
                    let ctx = crate::apply_model::heuristic::LocateContext {
                        ideal_line: 0,
                        cursor_scope: None,
                        named_scopes: &named,
                        allow_full_scan: true,
                    };
                    crate::observability::code_intel_metrics::incr_apply_hunk_with_anchor();
                    crate::apply_model::heuristic::apply_unified_diff_with_ctx(
                        &source, diff, &opts, &ctx,
                    )
                } else {
                    apply_unified_diff(&source, diff, &opts)
                }
                .map_err(|source| ApplyBatchError::Hunk {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                std::fs::write(path, outcome.applied.as_bytes()).map_err(|source| {
                    ApplyBatchError::Io {
                        op_index,
                        path: path.clone(),
                        source,
                    }
                })?;
                Ok((Some(before), Some(outcome.applied.len())))
            }
            EditOp::NotebookCell { path, cell: cell_op } => {
                let raw = std::fs::read_to_string(path).map_err(|source| {
                    ApplyBatchError::Io {
                        op_index,
                        path: path.clone(),
                        source,
                    }
                })?;
                let mut nb: serde_json::Value =
                    serde_json::from_str(&raw).map_err(|e| ApplyBatchError::Apply {
                        op_index,
                        path: path.clone(),
                        source: anyhow::anyhow!("Invalid JSON in notebook: {e}"),
                    })?;
                crate::tools::notebook_edit::apply_notebook_cell_op(&mut nb, cell_op).map_err(
                    |e| ApplyBatchError::Apply {
                        op_index,
                        path: path.clone(),
                        source: e,
                    },
                )?;
                let out = crate::tools::notebook_edit::notebook_to_string_pretty_one_space(&nb)
                    .map_err(|e| ApplyBatchError::Apply {
                        op_index,
                        path: path.clone(),
                        source: e,
                    })?;
                std::fs::write(path, out.as_bytes()).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                let _ = cell_op_label(cell_op);
                Ok((Some(raw.len()), Some(out.len())))
            }
            }
        })
        .await;
        match join {
            Ok(res) => res,
            Err(e) => Err(ApplyBatchError::Io {
                op_index,
                path: PathBuf::new(),
                source: std::io::Error::other(format!("apply_one join error: {e}")),
            }),
        }
    }

    async fn build_preview(
        &self,
        batch: &EditBatch,
    ) -> Result<BatchPreview, ApplyBatchError> {
        let batch = batch.clone();
        let apply_opts = self.apply_opts.clone();
        let join = tokio::task::spawn_blocking(move || {
            let batch = &batch;
            let apply_opts = &apply_opts;
        let mut diffs: Vec<UnifiedDiffPreview> = Vec::with_capacity(batch.ops.len());
        let mut created: Vec<PathBuf> = Vec::new();
        let mut deleted: Vec<PathBuf> = Vec::new();
        let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();
        for (idx, op) in batch.ops.iter().enumerate() {
            match op {
                EditOp::Replace {
                    path,
                    byte_range,
                    new_text,
                    ..
                } => {
                    let bytes = std::fs::read(path).unwrap_or_default();
                    let range = clamp_byte_range(byte_range, bytes.len());
                    let before_text = String::from_utf8_lossy(&bytes).to_string();
                    let mut after_bytes = Vec::with_capacity(bytes.len());
                    after_bytes.extend_from_slice(&bytes[..range.start]);
                    after_bytes.extend_from_slice(new_text.as_bytes());
                    after_bytes.extend_from_slice(&bytes[range.end..]);
                    let after_text = String::from_utf8_lossy(&after_bytes).to_string();
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: path.clone(),
                        unified_diff: render_minimal_unified_diff(
                            path, &before_text, &after_text,
                        ),
                        before_bytes: Some(bytes.len()),
                        after_bytes: Some(after_bytes.len()),
                    });
                }
                EditOp::Insert { path, at_byte, text, .. } => {
                    let bytes = std::fs::read(path).unwrap_or_default();
                    let at = (*at_byte).min(bytes.len());
                    let mut after = Vec::with_capacity(bytes.len() + text.len());
                    after.extend_from_slice(&bytes[..at]);
                    after.extend_from_slice(text.as_bytes());
                    after.extend_from_slice(&bytes[at..]);
                    let before_text = String::from_utf8_lossy(&bytes).to_string();
                    let after_text = String::from_utf8_lossy(&after).to_string();
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: path.clone(),
                        unified_diff: render_minimal_unified_diff(
                            path, &before_text, &after_text,
                        ),
                        before_bytes: Some(bytes.len()),
                        after_bytes: Some(after.len()),
                    });
                }
                EditOp::Delete { path, byte_range, .. } => {
                    let bytes = std::fs::read(path).unwrap_or_default();
                    let range = clamp_byte_range(byte_range, bytes.len());
                    let mut after = Vec::with_capacity(bytes.len());
                    after.extend_from_slice(&bytes[..range.start]);
                    after.extend_from_slice(&bytes[range.end..]);
                    let before_text = String::from_utf8_lossy(&bytes).to_string();
                    let after_text = String::from_utf8_lossy(&after).to_string();
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: path.clone(),
                        unified_diff: render_minimal_unified_diff(
                            path, &before_text, &after_text,
                        ),
                        before_bytes: Some(bytes.len()),
                        after_bytes: Some(after.len()),
                    });
                }
                EditOp::CreateFile { path, contents, .. } => {
                    created.push(path.clone());
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: path.clone(),
                        unified_diff: render_minimal_unified_diff(path, "", contents),
                        before_bytes: None,
                        after_bytes: Some(contents.len()),
                    });
                }
                EditOp::DeleteFile { path, .. } => {
                    let bytes = std::fs::read(path).unwrap_or_default();
                    let before_text = String::from_utf8_lossy(&bytes).to_string();
                    deleted.push(path.clone());
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: path.clone(),
                        unified_diff: render_minimal_unified_diff(path, &before_text, ""),
                        before_bytes: Some(bytes.len()),
                        after_bytes: None,
                    });
                }
                EditOp::RenameFile { from, to, .. } => {
                    renamed.push((from.clone(), to.clone()));
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: to.clone(),
                        unified_diff: format!(
                            "rename from {}\nrename to {}\n",
                            from.display(),
                            to.display()
                        ),
                        before_bytes: None,
                        after_bytes: None,
                    });
                }
                EditOp::ApplyHunk {
                    path,
                    diff,
                    fuzz,
                    scope_anchor,
                } => {
                    let source = std::fs::read_to_string(path).unwrap_or_default();
                    let mut opts = apply_opts.clone();
                    opts.max_fuzz = *fuzz as usize;
                    opts.dry_run = true;
                    let outcome = if let Some(anchor) = scope_anchor.as_ref() {
                        let named = build_named_scopes_for(path, anchor);
                        let ctx = crate::apply_model::heuristic::LocateContext {
                            ideal_line: 0,
                            cursor_scope: None,
                            named_scopes: &named,
                            allow_full_scan: true,
                        };
                        crate::apply_model::heuristic::apply_unified_diff_with_ctx(
                            &source, diff, &opts, &ctx,
                        )
                    } else {
                        apply_unified_diff(&source, diff, &opts)
                    }
                    .map_err(|source| ApplyBatchError::Hunk {
                        op_index: idx,
                        path: path.clone(),
                        source,
                    })?;
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: path.clone(),
                        unified_diff: diff.clone(),
                        before_bytes: Some(source.len()),
                        after_bytes: Some(outcome.applied.len()),
                    });
                }
                EditOp::NotebookCell { path, cell } => {
                    diffs.push(UnifiedDiffPreview {
                        op_index: idx,
                        path: path.clone(),
                        unified_diff: format!("notebook_cell:{}\n", cell_op_label(cell)),
                        before_bytes: None,
                        after_bytes: None,
                    });
                }
            }
        }
        Ok(BatchPreview {
            batch_id: batch.batch_id.clone(),
            diffs,
            created,
            deleted,
            renamed,
        })
        })
        .await;
        match join {
            Ok(res) => res,
            Err(e) => Err(ApplyBatchError::Io {
                op_index: 0,
                path: PathBuf::new(),
                source: std::io::Error::other(format!("build_preview join error: {e}")),
            }),
        }
    }

    async fn write_journal_pending(
        &self,
        batch: &EditBatch,
        pre_images: Arc<BTreeMap<PathBuf, PreImage>>,
    ) -> Result<(Option<PathBuf>, bool), ApplyBatchError> {
        let journaled = self.journal_dir_snapshot();
        let ws_snap = self.workspace_snapshot();
        let batch = batch.clone();
        let join = tokio::task::spawn_blocking(move || {
            if std::fs::create_dir_all(&journaled).is_err() {
                return Ok((None, false));
            }
            let path = journaled.join(format!("{}.jsonl", batch.batch_id));
            let header = JournalHeader {
                batch_id: batch.batch_id.clone(),
                correlation_id: batch.correlation_id.clone(),
                origin: batch.origin.tag().to_string(),
                atomic: batch.atomic,
                started_at: Utc::now(),
                workspace_root: ws_snap,
                status: JournalStatus::Pending,
            };
            let mut buf = String::new();
            buf.push_str(
                &serde_json::to_string(&JournalLine {
                    kind: JournalLineKind::Header,
                    header: Some(header),
                    record: None,
                    footer: None,
                })
                .map_err(|e| ApplyBatchError::Journal(std::io::Error::other(e.to_string())))?,
            );
            buf.push('\n');
            for (idx, op) in batch.ops.iter().enumerate() {
                let touched = op.primary_path().to_path_buf();
                let pre_image = pre_images.get(&touched).cloned();
                let record = JournalRecord {
                    op_index: idx,
                    op: op.clone(),
                    pre_image,
                    post_image_sha256: None,
                    ts: Utc::now(),
                };
                buf.push_str(
                    &serde_json::to_string(&JournalLine {
                        kind: JournalLineKind::Record,
                        header: None,
                        record: Some(record),
                        footer: None,
                    })
                    .map_err(|e| ApplyBatchError::Journal(std::io::Error::other(e.to_string())))?,
                );
                buf.push('\n');
            }
            std::fs::write(&path, buf.as_bytes()).map_err(ApplyBatchError::Journal)?;
            Ok((Some(path), true))
        })
        .await;
        match join {
            Ok(res) => res,
            Err(e) => Err(ApplyBatchError::Journal(std::io::Error::other(format!(
                "write_journal_pending join error: {e}"
            )))),
        }
    }

    fn append_footer(&self, path: Option<&Path>, status: JournalStatus, degraded: bool) {
        let Some(path) = path else { return };
        append_footer_to_path(path, status, degraded);
    }

    fn rotate_journals(&self) {
        let Ok(entries) = std::fs::read_dir(self.journal_dir_snapshot()) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                    let m = e.metadata().ok().and_then(|m| m.modified().ok())?;
                    Some((m, p))
                } else {
                    None
                }
            })
            .collect();
        if files.len() <= self.journal_retention {
            return;
        }
        files.sort_by_key(|(t, _)| *t);
        let to_remove = files.len() - self.journal_retention;
        for (_, path) in files.into_iter().take(to_remove) {
            let _ = std::fs::remove_file(path);
        }
    }

}

fn build_named_scopes_for(
    path: &Path,
    anchor: &crate::apply_model::edit_op::ScopeAnchor,
) -> Vec<crate::apply_model::heuristic::NamedScope> {
    use crate::apply_model::heuristic::NamedScope;
    use std::ops::Range;

    let kind = scope_kind_from_str(&anchor.kind);
    if let Some(byte_range) = anchor.byte_range.clone() {

        let line_range = Range {
            start: 1,
            end: 1,
        };
        return vec![NamedScope {
            kind,
            name: anchor.name.clone(),
            byte_range,
            line_range,
        }];
    }

    let Ok(src) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let entries = match crate::code_intel::outline::extract_outline(path, None) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let total_lines = src.lines().count().max(1);
    let matches: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name == anchor.name && e.kind == anchor.kind)
        .map(|(i, _)| i)
        .collect();
    matches
        .into_iter()
        .map(|idx| {
            let entry = &entries[idx];
            let start_line = entry.line as usize;
            let end_line = entries
                .get(idx + 1)
                .map(|n| (n.line as usize).saturating_sub(1))
                .unwrap_or(total_lines);
            let byte_range = line_range_to_byte_range(&src, start_line, end_line);
            NamedScope {
                kind,
                name: entry.name.clone(),
                byte_range,
                line_range: Range {
                    start: start_line,
                    end: end_line.max(start_line),
                },
            }
        })
        .collect()
}

fn scope_kind_from_str(kind: &str) -> crate::apply_model::edit_op::ScopeKind {
    use crate::apply_model::edit_op::ScopeKind;
    match kind.to_ascii_lowercase().as_str() {
        "function" | "fn" | "method" => ScopeKind::Function,
        "class" | "struct" | "enum" => ScopeKind::Class,
        "module" | "mod" => ScopeKind::Module,
        "block" => ScopeKind::Block,
        _ => ScopeKind::Other,
    }
}

fn validate_byte_range_for_apply(
    op_index: usize,
    path: &std::path::Path,
    byte_range: &std::ops::Range<usize>,
    len: usize,
) -> Result<(), ApplyBatchError> {
    if byte_range.start > byte_range.end || byte_range.end > len {
        return Err(ApplyBatchError::Apply {
            op_index,
            path: path.to_path_buf(),
            source: anyhow::anyhow!(
                "stale byte range {}..{} for file of {} byte(s); the file changed since this \
                 edit was computed. Re-read the file and recompute the edit before retrying.",
                byte_range.start,
                byte_range.end,
                len
            ),
        });
    }
    Ok(())
}

fn validate_at_byte_for_apply(
    op_index: usize,
    path: &std::path::Path,
    at_byte: usize,
    len: usize,
) -> Result<(), ApplyBatchError> {
    if at_byte > len {
        return Err(ApplyBatchError::Apply {
            op_index,
            path: path.to_path_buf(),
            source: anyhow::anyhow!(
                "stale insert offset {} for file of {} byte(s); the file changed since this \
                 edit was computed. Re-read the file and recompute the edit before retrying.",
                at_byte,
                len
            ),
        });
    }
    Ok(())
}

fn clamp_byte_range(byte_range: &std::ops::Range<usize>, len: usize) -> std::ops::Range<usize> {
    let start = byte_range.start.min(len);
    let end = byte_range.end.clamp(start, len);
    start..end
}

fn line_range_to_byte_range(
    source: &str,
    start_line_1: usize,
    end_line_1: usize,
) -> std::ops::Range<usize> {
    let start_line = start_line_1.saturating_sub(1);
    let end_line = end_line_1.max(start_line_1);
    let mut start_byte: usize = 0;
    let mut end_byte: usize = source.len();
    let mut line_no: usize = 0;
    let mut cursor: usize = 0;
    for line in source.split_inclusive('\n') {
        if line_no == start_line {
            start_byte = cursor;
        }
        cursor += line.len();
        line_no += 1;
        if line_no == end_line {
            end_byte = cursor;
            break;
        }
    }
    start_byte..end_byte
}

fn unique_touched_paths(batch: &EditBatch) -> Vec<PathBuf> {
    let mut set = std::collections::BTreeSet::new();
    for op in &batch.ops {
        for p in op.touched_paths() {
            set.insert(p.to_path_buf());
        }
    }
    set.into_iter().collect()
}

fn region_requests_for_batch(batch: &EditBatch) -> Vec<RegionLockRequest> {
    let mut out: Vec<RegionLockRequest> = Vec::with_capacity(batch.ops.len());
    for op in &batch.ops {
        match op {
            EditOp::Replace {
                path, byte_range, ..
            } => out.push(RegionLockRequest {
                path: path.clone(),
                range: byte_range.clone(),
                exclusive: true,
            }),
            EditOp::Insert { path, at_byte, .. } => out.push(RegionLockRequest {
                path: path.clone(),

                range: *at_byte..at_byte.saturating_add(1),
                exclusive: true,
            }),
            EditOp::Delete {
                path, byte_range, ..
            } => out.push(RegionLockRequest {
                path: path.clone(),
                range: byte_range.clone(),
                exclusive: true,
            }),
            EditOp::ApplyHunk { path, .. } => out.push(RegionLockRequest {
                path: path.clone(),
                range: 0..usize::MAX,
                exclusive: true,
            }),
            EditOp::CreateFile { path, .. }
            | EditOp::DeleteFile { path, .. }
            | EditOp::NotebookCell { path, .. } => out.push(RegionLockRequest {
                path: path.clone(),
                range: 0..usize::MAX,
                exclusive: true,
            }),
            EditOp::RenameFile { from, to, .. } => {
                out.push(RegionLockRequest {
                    path: from.clone(),
                    range: 0..usize::MAX,
                    exclusive: true,
                });
                out.push(RegionLockRequest {
                    path: to.clone(),
                    range: 0..usize::MAX,
                    exclusive: true,
                });
            }
        }
    }
    out
}

fn capture_pre_images(
    batch: &EditBatch,
) -> Result<BTreeMap<PathBuf, PreImage>, ApplyBatchError> {
    let mut map: BTreeMap<PathBuf, PreImage> = BTreeMap::new();
    for op in &batch.ops {
        let path = op.primary_path().to_path_buf();
        if !map.contains_key(&path) {
            let bytes = std::fs::read(&path).ok();
            let mtime_ms = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64);
            let sha256 = bytes.as_ref().map(|b| {
                let mut hasher = Sha256::new();
                hasher.update(b);
                format!("{:x}", hasher.finalize())
            });

            let rename_target_bytes = if let EditOp::RenameFile { to, .. } = op {
                std::fs::read(to).ok()
            } else {
                None
            };
            map.insert(
                path.clone(),
                PreImage {
                    path,
                    bytes,
                    rename_target_bytes,
                    sha256,
                    mtime_ms,
                },
            );
        }
    }
    Ok(map)
}

fn append_footer_to_path(path: &Path, status: JournalStatus, degraded: bool) {
    let footer = JournalLine {
        kind: JournalLineKind::Footer,
        header: None,
        record: None,
        footer: Some(JournalFooter {
            status,
            finished_at: Utc::now(),
            degraded,
        }),
    };
    if let Ok(line) = serde_json::to_string(&footer) {
        if let Ok(mut existing) = std::fs::read_to_string(path) {
            existing.push_str(&line);
            existing.push('\n');
            let _ = std::fs::write(path, existing.as_bytes());
        }
    }
}

fn restore_pre_images(map: &BTreeMap<PathBuf, PreImage>) -> Result<(), std::io::Error> {

    for pre in map.values() {
        restore_one(pre)?;
    }
    Ok(())
}

fn restore_one(pre: &PreImage) -> Result<(), std::io::Error> {
    match &pre.bytes {
        Some(bytes) => {
            if let Some(parent) = pre.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&pre.path, bytes)
        }
        None => match std::fs::remove_file(&pre.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        },
    }
}

fn render_minimal_unified_diff(path: &Path, before: &str, after: &str) -> String {
    let mut out = format!("--- a/{}\n+++ b/{}\n", path.display(), path.display());
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    out.push_str(&format!(
        "@@ -1,{} +1,{} @@\n",
        before_lines.len().max(1),
        after_lines.len().max(1)
    ));
    for line in &before_lines {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    for line in &after_lines {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn cell_op_label(op: &NotebookCellOp) -> &'static str {
    match op {
        NotebookCellOp::Replace { .. } => "replace",
        NotebookCellOp::Insert { .. } => "insert",
        NotebookCellOp::Delete { .. } => "delete",
    }
}

impl std::fmt::Debug for OpsApplier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpsApplier")
            .field("workspace_root", &self.workspace_snapshot())
            .field("journal_dir", &self.journal_dir_snapshot())
            .field("journal_retention", &self.journal_retention)
            .finish_non_exhaustive()
    }
}
