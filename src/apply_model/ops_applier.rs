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

mod journal_bytes {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => serializer
                .serialize_some(&base64::engine::general_purpose::STANDARD.encode(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Text(String),
            Raw(Vec<u8>),
        }
        Ok(match Option::<Repr>::deserialize(deserializer)? {
            None => None,
            Some(Repr::Raw(v)) => Some(v),
            Some(Repr::Text(t)) => Some(
                base64::engine::general_purpose::STANDARD
                    .decode(t.as_bytes())
                    .map_err(serde::de::Error::custom)?,
            ),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PreImage {

    path: PathBuf,

    #[serde(default, with = "journal_bytes")]
    bytes: Option<Vec<u8>>,

    #[serde(default, with = "journal_bytes")]
    rename_target_bytes: Option<Vec<u8>>,
    #[serde(default)]
    rename_from: Option<PathBuf>,
    #[serde(default, with = "journal_bytes")]
    rename_from_bytes: Option<Vec<u8>>,
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
    allowed_roots: Vec<PathBuf>,
    lock_provider: Arc<dyn LockProvider>,
    validator: Arc<dyn BatchValidator>,
    validator_is_noop: bool,
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

struct LazyServiceLspNotifier;

#[async_trait::async_trait]
impl LspNotifier for LazyServiceLspNotifier {
    async fn notify_changed(&self, path: &Path, contents: &str) -> anyhow::Result<()> {
        let Some(svc) = crate::services::try_get_services() else {
            return Ok(());
        };
        svc.lsp.notify_file_changed(path, contents).await
    }
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
    pub fn locked_for_workspace(workspace_root: impl Into<PathBuf>) -> Self {
        let raw = workspace_root.into();
        let canon = std::fs::canonicalize(&raw).unwrap_or(raw);
        let history_root = if canon.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            canon.clone()
        };
        let lock_provider: Arc<dyn LockProvider> = Arc::new(
            crate::apply_model::lock_manager_provider::LazyRuntimeLockProvider::new("edit_path"),
        );
        let history = crate::tools::edit_history::EditHistory::shared_for_workspace(&history_root);
        Self::default_for_shared_workspace(Arc::new(RwLock::new(canon)))
            .with_lock_provider(lock_provider)
            .with_edit_history(history)
    }

    #[must_use]
    pub fn default_for_shared_workspace(workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        Self {
            workspace_root,
            allowed_roots: Vec::new(),
            lock_provider: Arc::new(NoopLockProvider),
            validator: Arc::new(NoopBatchValidator),
            validator_is_noop: true,
            apply_opts: ApplyOptions::default(),
            journal_retention: 64,
            symbol_graph_writer: None,
            lsp_notify: Some(Arc::new(LazyServiceLspNotifier)),
            edit_history: None,
        }
    }

    #[must_use]
    pub fn with_allowed_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.allowed_roots = roots
            .into_iter()
            .map(|root| std::fs::canonicalize(&root).unwrap_or(root))
            .collect();
        self
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
        self.validator_is_noop = false;
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
        self.recover_pending_journals_once().await;

        if let Some(dup) = first_conflicting_multi_op_path(&batch) {
            return Err(ApplyBatchError::Apply {
                op_index: 0,
                path: dup.clone(),
                source: anyhow::anyhow!(
                    "batch contains multiple byte-offset edits to the same file ({}); \
                     merge them into a single op before applying (offsets would collide)",
                    dup.display()
                ),
            });
        }

        let unique_paths = unique_touched_paths(&batch);
        let region_requests = region_requests_for_batch(&batch);
        let _guard = self
            .lock_provider
            .acquire_for_regions(&region_requests, &batch.origin.holder_tag())
            .await?;

        {
            let batch_for_validate = batch.clone();
            let allowed_roots = self.allowed_roots.clone();
            let validate = tokio::task::spawn_blocking(move || -> Result<(), ApplyBatchError> {
                for op in &batch_for_validate.ops {
                    op.validate_preconditions_with_roots(&ws, &allowed_roots)?;
                }
                Ok(())
            })
            .await;
            match validate {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(e) => {
                    return Err(ApplyBatchError::Io {
                        op_index: 0,
                        path: PathBuf::new(),
                        source: std::io::Error::other(format!(
                            "validate_preconditions join error: {e}"
                        )),
                    });
                }
            }
        }

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

        let op_serial: Arc<std::sync::Mutex<()>> = Arc::new(std::sync::Mutex::new(()));
        let mut torn_guard = TornBatchGuard {
            pre_images: Arc::clone(&pre_images),
            journal_path: journal_path.clone(),
            atomic: batch.atomic,
            batch_id: batch.batch_id.clone(),
            op_serial: Arc::clone(&op_serial),
            armed: true,
        };

        let preview = if self.validator_is_noop {
            BatchPreview {
                batch_id: batch.batch_id.clone(),
                diffs: Vec::new(),
                created: Vec::new(),
                deleted: Vec::new(),
                renamed: Vec::new(),
            }
        } else {
            self.build_preview(&batch).await?
        };

        let mut per_op: Vec<OpOutcome> = Vec::with_capacity(batch.ops.len());
        let mut degraded = false;
        let mut applied_paths: Vec<PathBuf> = Vec::new();
        let mut post_images: Vec<(usize, String)> = Vec::with_capacity(batch.ops.len());

        for (idx, op) in batch.ops.iter().enumerate() {
            let touched = op.primary_path().to_path_buf();
            match self.apply_one(idx, op, Arc::clone(&op_serial)).await {
                Ok((before, after, post_sha)) => {
                    if let Some(sha) = post_sha {
                        post_images.push((idx, sha));
                    }
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
                        torn_guard.disarm();
                        if let Err(rb) = restore_pre_images_async(&pre_images).await {
                            self.finalize_journal_async(
                                journal_path.clone(),
                                JournalStatus::RolledBack,
                                true,
                            )
                            .await;
                            return Err(ApplyBatchError::RollbackFailed {
                                primary: msg,
                                rollback: rb.to_string(),
                            });
                        }
                        self.finalize_journal_async(
                            journal_path.clone(),
                            JournalStatus::RolledBack,
                            false,
                        )
                        .await;
                        return Err(err);
                    }
                    degraded = true;
                }
            }
        }

        if let Err(verr) = self.validator.validate(&batch, &preview).await {
            if batch.atomic {
                torn_guard.disarm();
                if let Err(rb) = restore_pre_images_async(&pre_images).await {
                    self.finalize_journal_async(
                        journal_path.clone(),
                        JournalStatus::RolledBack,
                        true,
                    )
                    .await;
                    return Err(ApplyBatchError::RollbackFailed {
                        primary: verr.to_string(),
                        rollback: rb.to_string(),
                    });
                }
                self.finalize_journal_async(
                    journal_path.clone(),
                    JournalStatus::RolledBack,
                    false,
                )
                .await;
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
                    let before_text: Option<String> = pre_images
                        .get(path)
                        .and_then(|pre| pre.bytes.as_ref())
                        .map(|b| String::from_utf8_lossy(b).into_owned());
                    let report =
                        super::validator::validate_edit(before_text.as_deref(), &text, Some(path));
                    if report.is_confident_failure() {
                        if batch.atomic {
                            torn_guard.disarm();
                            let _ = restore_pre_images_async(&pre_images).await;
                            self.finalize_journal_async(
                                journal_path.clone(),
                                JournalStatus::RolledBack,
                                false,
                            )
                            .await;
                            return Err(ApplyBatchError::Validator(
                                BatchValidatorError::Rejected(format!(
                                    "{}: {}",
                                    path.display(),
                                    report.advisory_summary()
                                )),
                            ));
                        }
                        degraded = true;
                    } else if !report.is_ok() {
                        tracing::debug!(
                            target: "apply_model.ops_applier",
                            path = %path.display(),
                            issues = %report.advisory_summary(),
                            "post-write validation produced advisory warnings; not rolling back"
                        );
                    }
                }
            }
        }

        let post_hashes_by_path: std::collections::HashMap<PathBuf, String> = post_images
            .iter()
            .filter_map(|(idx, sha)| {
                batch
                    .ops
                    .get(*idx)
                    .map(|op| (op.primary_path().to_path_buf(), sha.clone()))
            })
            .collect();

        torn_guard.disarm();
        self.finalize_journal_committed(
            journal_path.clone(),
            degraded,
            std::mem::take(&mut post_images),
        )
        .await;

        if let Some(history) = self.edit_history.as_ref() {
            history.stamp_latest_with_batch(
                unique_paths.iter(),
                &batch.batch_id,
                &post_hashes_by_path,
            );
        }

        if let Some(writer) = self.symbol_graph_writer.as_ref() {
            let changed: Vec<PathBuf> = unique_paths.to_vec();
            writer.on_files_changed(&changed);
        } else {
            crate::code_intel::symbol_graph::incremental::note_files_changed_global(
                &unique_paths,
            );
        }

        crate::agent::loop_::services::note_code_files_changed(&unique_paths);

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
        let (source, encoding_label) = {
            let path_for_read = path.clone();
            let raw_bytes = tokio::task::spawn_blocking(move || std::fs::read(&path_for_read))
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
                })?;
            if crate::tools::file::encoding::is_probably_binary(&raw_bytes) {
                return Err(ApplyBatchError::Io {
                    op_index: 0,
                    path: path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "refusing to apply a text diff to a binary file",
                    ),
                });
            }
            let (text, label) = crate::tools::file::encoding::decode_for_edit(&raw_bytes)
                .map_err(|source| ApplyBatchError::Io {
                    op_index: 0,
                    path: path.clone(),
                    source,
                })?;
            (text, label)
        };
        let mut options = options.clone();
        if options.path.is_none() {
            options.path = Some(path.clone());
        }
        let (outcome, _final_diff, tier) =
            super::fast_apply::apply_unified_diff_with_fast_path(
                &source, raw_diff, &options, refiner, hint,
            )
            .await
            .map_err(|e| ApplyBatchError::Hunk {
                op_index: 0,
                path: path.clone(),
                source: e,
            })?;
        let new_text = outcome.applied;
        let op = if crate::tools::file::encoding::is_utf8_label(encoding_label) {
            let len_before = source.len();
            super::edit_op::EditOp::Replace {
                path: path.clone(),
                byte_range: 0..len_before,
                old_text: source,
                new_text,
                anchor: None,
            }
        } else {
            super::edit_op::EditOp::CreateFile {
                path: path.clone(),
                contents: new_text,
                overwrite: true,
                encoding: Some(encoding_label.to_string()),
            }
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
            op.validate_preconditions_with_roots(&ws, &self.allowed_roots)?;
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
            let mut last_footer_status: Option<JournalStatus> = None;
            for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                let parsed: JournalLine = serde_json::from_str(line)
                    .map_err(|e| RollbackError::Parse(e.to_string()))?;
                match parsed.kind {
                    JournalLineKind::Record => {
                        if let Some(rec) = parsed.record {
                            records.push(rec);
                        }
                    }
                    JournalLineKind::Footer => {
                        if let Some(footer) = parsed.footer {
                            last_footer_status = Some(footer.status);
                        }
                    }
                    JournalLineKind::Header => {}
                }
            }

            if last_footer_status == Some(JournalStatus::RolledBack) {
                return Ok(());
            }

            for record in records.into_iter().rev() {
                if let Some(pre) = &record.pre_image {
                    if !post_image_is_fresh(&pre.path, record.post_image_sha256.as_deref()) {
                        tracing::warn!(
                            target: "apply_model.ops_applier",
                            path = %pre.path.display(),
                            "skipping rollback of a file that changed after this batch was applied \
                             (post-image mismatch); refusing to clobber newer content"
                        );
                        continue;
                    }
                    restore_one(pre).map_err(|source| RollbackError::Io {
                        path: pre.path.clone(),
                        source,
                    })?;
                } else if let EditOp::RenameFile { from, to, .. } = &record.op {
                    let _ = std::fs::rename(to, from);
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
        op_serial: Arc<std::sync::Mutex<()>>,
    ) -> Result<(Option<usize>, Option<usize>, Option<String>), ApplyBatchError> {
        let op = op.clone();
        let apply_opts = self.apply_opts.clone();
        #[cfg(feature = "crdt-coordination")]
        let (crdt_site, crdt_workspace_key) = crate::crdt::coordination_identity();
        let join = tokio::task::spawn_blocking(move || {
            let _serial = op_serial
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let op = &op;
            let apply_opts = &apply_opts;
            match op {
            EditOp::Replace {
                path,
                byte_range,
                old_text,
                new_text,
                ..
            } => {
                #[cfg(feature = "crdt-coordination")]
                let remote_merged =
                    crate::crdt::pull_remote_before_edit(path, &crdt_site, &crdt_workspace_key);
                #[cfg(not(feature = "crdt-coordination"))]
                let remote_merged = false;
                let bytes = std::fs::read(path).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                let before = bytes.len();
                validate_byte_range_for_apply(op_index, path, byte_range, before)?;
                if remote_merged && old_text.is_empty() {
                    return Err(ApplyBatchError::Apply {
                        op_index,
                        path: path.clone(),
                        source: anyhow::anyhow!(
                            "concurrent remote edit merged into this file; byte offsets are stale and the op carries no old_text to verify against; re-read the file and retry"
                        ),
                    });
                }
                if !old_text.is_empty()
                    && bytes.get(byte_range.start..byte_range.end)
                        != Some(old_text.as_bytes())
                {
                    return Err(ApplyBatchError::Apply {
                        op_index,
                        path: path.clone(),
                        source: anyhow::anyhow!(
                            "stale byte range: file content at {}..{} no longer matches the op's old_text (file changed since the range was computed); re-read the file and retry",
                            byte_range.start,
                            byte_range.end
                        ),
                    });
                }
                let mut out = Vec::with_capacity(
                    before - (byte_range.end - byte_range.start) + new_text.len(),
                );
                out.extend_from_slice(&bytes[..byte_range.start]);
                out.extend_from_slice(new_text.as_bytes());
                out.extend_from_slice(&bytes[byte_range.end..]);
                atomic_write(path, &out).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                #[cfg(feature = "crdt-coordination")]
                {
                    if let Err(e) =
                        crate::crdt::observe_after_disk_write(op, &crdt_site, &crdt_workspace_key)
                    {
                        tracing::error!(
                            target: "apply_model.ops_applier",
                            path = %path.display(),
                            error = %e,
                            "crdt observe_after_disk_write failed: the disk write succeeded but the op did not reach the shared log; marking document for full snapshot resync"
                        );
                        crate::crdt::mark_needs_resync(path, &crdt_site, &crdt_workspace_key);
                    }
                }
                let post = sha256_hex(&out);
                Ok((Some(before), Some(out.len()), Some(post)))
            }
            EditOp::Insert { path, at_byte, text, .. } => {
                #[cfg(feature = "crdt-coordination")]
                if crate::crdt::pull_remote_before_edit(path, &crdt_site, &crdt_workspace_key) {
                    return Err(ApplyBatchError::Apply {
                        op_index,
                        path: path.clone(),
                        source: anyhow::anyhow!(
                            "concurrent remote edit merged into this file; insertion offset is stale; re-read the file and retry"
                        ),
                    });
                }
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
                atomic_write(path, &out).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                #[cfg(feature = "crdt-coordination")]
                {
                    if let Err(e) =
                        crate::crdt::observe_after_disk_write(op, &crdt_site, &crdt_workspace_key)
                    {
                        tracing::error!(
                            target: "apply_model.ops_applier",
                            path = %path.display(),
                            error = %e,
                            "crdt observe_after_disk_write failed: the disk write succeeded but the op did not reach the shared log; marking document for full snapshot resync"
                        );
                        crate::crdt::mark_needs_resync(path, &crdt_site, &crdt_workspace_key);
                    }
                }
                let post = sha256_hex(&out);
                Ok((Some(before), Some(out.len()), Some(post)))
            }
            EditOp::Delete {
                path,
                byte_range,
                old_text,
                ..
            } => {
                #[cfg(feature = "crdt-coordination")]
                if crate::crdt::pull_remote_before_edit(path, &crdt_site, &crdt_workspace_key) {
                    return Err(ApplyBatchError::Apply {
                        op_index,
                        path: path.clone(),
                        source: anyhow::anyhow!(
                            "concurrent remote edit merged into this file; deletion range is stale; re-read the file and retry"
                        ),
                    });
                }
                let bytes = std::fs::read(path).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                let before = bytes.len();
                validate_byte_range_for_apply(op_index, path, byte_range, before)?;
                if let Some(expected) = old_text.as_ref() {
                    if &bytes[byte_range.clone()] != expected.as_bytes() {
                        return Err(ApplyBatchError::Apply {
                            op_index,
                            path: path.clone(),
                            source: anyhow::anyhow!(
                                "delete range content no longer matches expected old_text; \
                                 file changed since the range was computed; re-read and retry"
                            ),
                        });
                    }
                }
                let mut out =
                    Vec::with_capacity(before - (byte_range.end - byte_range.start));
                out.extend_from_slice(&bytes[..byte_range.start]);
                out.extend_from_slice(&bytes[byte_range.end..]);
                atomic_write(path, &out).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                #[cfg(feature = "crdt-coordination")]
                {
                    if let Err(e) =
                        crate::crdt::observe_after_disk_write(op, &crdt_site, &crdt_workspace_key)
                    {
                        tracing::error!(
                            target: "apply_model.ops_applier",
                            path = %path.display(),
                            error = %e,
                            "crdt observe_after_disk_write failed: the disk write succeeded but the op did not reach the shared log; marking document for full snapshot resync"
                        );
                        crate::crdt::mark_needs_resync(path, &crdt_site, &crdt_workspace_key);
                    }
                }
                let post = sha256_hex(&out);
                Ok((Some(before), Some(out.len()), Some(post)))
            }
            EditOp::CreateFile {
                path,
                contents,
                overwrite,
                encoding,
            } => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| ApplyBatchError::Io {
                        op_index,
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                let out_bytes: Vec<u8> = match encoding.as_deref() {
                    Some(label)
                        if !crate::tools::file::encoding::is_utf8_label(label) =>
                    {
                        match crate::tools::file::encoding::encode_with_label(label, contents) {
                            Some(bytes) => bytes,
                            None => {
                                return Err(ApplyBatchError::Io {
                                    op_index,
                                    path: path.clone(),
                                    source: std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!(
                                            "content cannot be represented in requested encoding {label}"
                                        ),
                                    ),
                                });
                            }
                        }
                    }
                    _ => contents.as_bytes().to_vec(),
                };
                let write_result = if *overwrite {
                    atomic_write(path, &out_bytes)
                } else {
                    crate::util::atomic_write_new(path, &out_bytes)
                };
                write_result.map_err(|source| {
                    ApplyBatchError::Io {
                        op_index,
                        path: path.clone(),
                        source,
                    }
                })?;
                #[cfg(feature = "crdt-coordination")]
                crate::crdt::invalidate(path);
                let post = sha256_hex(&out_bytes);
                Ok((None, Some(out_bytes.len()), Some(post)))
            }
            EditOp::DeleteFile { path, missing_ok } => match std::fs::remove_file(path) {
                Ok(()) => {
                    #[cfg(feature = "crdt-coordination")]
                    crate::crdt::invalidate(path);
                    Ok((None, None, None))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && *missing_ok => {
                    #[cfg(feature = "crdt-coordination")]
                    crate::crdt::invalidate(path);
                    Ok((None, None, None))
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
                overwrite,
            } => {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| ApplyBatchError::Io {
                        op_index,
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                let rename_result = if *overwrite {
                    crate::util::atomic_replace_file(from, to)
                } else {
                    crate::util::atomic_move_no_replace(from, to)
                };
                match rename_result {
                    Ok(()) => {
                        #[cfg(feature = "crdt-coordination")]
                        {
                            crate::crdt::invalidate(from);
                            crate::crdt::invalidate(to);
                        }
                        let bytes = std::fs::read(to).map_err(|source| ApplyBatchError::Io {
                            op_index,
                            path: to.clone(),
                            source,
                        })?;
                        Ok((None, Some(bytes.len()), Some(sha256_hex(&bytes))))
                    }
                    Err(source) => {
                        if !*overwrite {
                            return Err(ApplyBatchError::Io {
                                op_index,
                                path: to.clone(),
                                source,
                            });
                        }
                        let bytes =
                            std::fs::read(from).map_err(|source| ApplyBatchError::Io {
                                op_index,
                                path: from.clone(),
                                source,
                            })?;
                        atomic_write(to, &bytes).map_err(|source| ApplyBatchError::Io {
                            op_index,
                            path: to.clone(),
                            source,
                        })?;
                        std::fs::remove_file(from).map_err(|source| ApplyBatchError::Io {
                            op_index,
                            path: from.clone(),
                            source,
                        })?;
                        #[cfg(feature = "crdt-coordination")]
                        {
                            crate::crdt::invalidate(from);
                            crate::crdt::invalidate(to);
                        }
                        Ok((None, Some(bytes.len()), Some(sha256_hex(&bytes))))
                    }
                }
            }
            EditOp::ApplyHunk {
                path,
                diff,
                fuzz,
                scope_anchor,
            } => {
                let raw_bytes = std::fs::read(path).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                if crate::tools::file::encoding::is_probably_binary(&raw_bytes) {
                    return Err(ApplyBatchError::Io {
                        op_index,
                        path: path.clone(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "refusing to apply a text hunk to a binary file",
                        ),
                    });
                }
                let (source, encoding_label) =
                    crate::tools::file::encoding::decode_for_edit(&raw_bytes).map_err(
                        |source| ApplyBatchError::Io {
                            op_index,
                            path: path.clone(),
                            source,
                        },
                    )?;
                let before = source.len();
                let mut opts = apply_opts.clone();
                opts.max_fuzz = *fuzz as usize;
                opts.dry_run = false;
                opts.path = Some(path.clone());

                let outcome = if let Some(anchor) = scope_anchor.as_ref() {
                    let named = build_named_scopes_for(path, anchor);
                    let ctx = crate::apply_model::heuristic::LocateContext {
                        ideal_line: 0,
                        cursor_scope: None,
                        named_scopes: &named,
                        allow_full_scan: false,
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
                let out_bytes = crate::tools::file::encoding::encode_with_label(
                    encoding_label,
                    &outcome.applied,
                )
                .ok_or_else(|| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "edited content cannot be represented in original encoding {encoding_label}"
                        ),
                    ),
                })?;
                atomic_write(path, &out_bytes).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                #[cfg(feature = "crdt-coordination")]
                {
                    crate::crdt::invalidate(path);
                    crate::crdt::mark_needs_resync(path, &crdt_site, &crdt_workspace_key);
                }
                let post = sha256_hex(&out_bytes);
                Ok((Some(before), Some(out_bytes.len()), Some(post)))
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
                atomic_write(path, out.as_bytes()).map_err(|source| ApplyBatchError::Io {
                    op_index,
                    path: path.clone(),
                    source,
                })?;
                #[cfg(feature = "crdt-coordination")]
                {
                    crate::crdt::invalidate(path);
                    crate::crdt::mark_needs_resync(path, &crdt_site, &crdt_workspace_key);
                }
                let _ = cell_op_label(cell_op);
                let post = sha256_hex(out.as_bytes());
                Ok((Some(raw.len()), Some(out.len()), Some(post)))
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
                    let raw = std::fs::read(path).unwrap_or_default();
                    let (source, _) =
                        crate::tools::file::encoding::decode_for_edit(&raw).map_err(
                            |source| ApplyBatchError::Io {
                                op_index: idx,
                                path: path.clone(),
                                source,
                            },
                        )?;
                    let mut opts = apply_opts.clone();
                    opts.max_fuzz = *fuzz as usize;
                    opts.dry_run = true;
                    let outcome = if let Some(anchor) = scope_anchor.as_ref() {
                        let named = build_named_scopes_for(path, anchor);
                        let ctx = crate::apply_model::heuristic::LocateContext {
                            ideal_line: 0,
                            cursor_scope: None,
                            named_scopes: &named,
                            allow_full_scan: false,
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
                        unified_diff: render_minimal_unified_diff(
                            path,
                            &source,
                            &outcome.applied,
                        ),
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
            atomic_write(&path, buf.as_bytes()).map_err(ApplyBatchError::Journal)?;
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

    async fn finalize_journal_async(
        &self,
        journal_path: Option<PathBuf>,
        status: JournalStatus,
        degraded: bool,
    ) {
        let dir = self.journal_dir_snapshot();
        let retention = self.journal_retention;
        let _ = tokio::task::spawn_blocking(move || {
            if let Some(p) = journal_path {
                append_footer_to_path(&p, status, degraded);
            }
            rotate_journals_in(&dir, retention);
        })
        .await;
    }

    async fn finalize_journal_committed(
        &self,
        journal_path: Option<PathBuf>,
        degraded: bool,
        post_images: Vec<(usize, String)>,
    ) {
        let dir = self.journal_dir_snapshot();
        let retention = self.journal_retention;
        let _ = tokio::task::spawn_blocking(move || {
            if let Some(p) = journal_path {
                if !post_images.is_empty() {
                    stamp_post_images_in_journal(&p, &post_images);
                }
                append_footer_to_path(&p, JournalStatus::Committed, degraded);
            }
            rotate_journals_in(&dir, retention);
        })
        .await;
    }

    async fn recover_pending_journals_once(&self) {
        let dir = self.journal_dir_snapshot();
        {
            let mut seen = RECOVERED_JOURNAL_DIRS.lock();
            if !seen.insert(dir.clone()) {
                return;
            }
        }
        let _ = tokio::task::spawn_blocking(move || recover_pending_journals_in(&dir)).await;
    }

}

static RECOVERED_JOURNAL_DIRS: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashSet<PathBuf>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashSet::new()));

struct TornBatchGuard {
    pre_images: Arc<BTreeMap<PathBuf, PreImage>>,
    journal_path: Option<PathBuf>,
    atomic: bool,
    batch_id: String,
    op_serial: Arc<std::sync::Mutex<()>>,
    armed: bool,
}

impl TornBatchGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TornBatchGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _serial = self
            .op_serial
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.atomic {
            match restore_pre_images(&self.pre_images) {
                Ok(()) => {
                    tracing::warn!(
                        target: "apply_model.ops_applier",
                        batch_id = %self.batch_id,
                        "atomic edit batch dropped mid-flight (cancelled or panicked); \
                         pre-images restored to prevent torn writes"
                    );
                    if let Some(p) = &self.journal_path {
                        append_footer_to_path(p, JournalStatus::RolledBack, false);
                    }
                }
                Err(e) => {
                    tracing::error!(
                        target: "apply_model.ops_applier",
                        batch_id = %self.batch_id,
                        error = %e,
                        "atomic edit batch dropped mid-flight and pre-image restore failed; \
                         pending journal kept for startup recovery"
                    );
                    if let Some(p) = &self.journal_path {
                        append_footer_to_path(p, JournalStatus::RolledBack, true);
                    }
                }
            }
        } else {
            tracing::warn!(
                target: "apply_model.ops_applier",
                batch_id = %self.batch_id,
                "edit batch dropped mid-flight; partial non-atomic results kept"
            );
            if let Some(p) = &self.journal_path {
                append_footer_to_path(p, JournalStatus::Committed, true);
            }
        }
    }
}

fn recover_pending_journals_in(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut header: Option<JournalHeader> = None;
        let mut records: Vec<JournalRecord> = Vec::new();
        let mut has_footer = false;
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(parsed) = serde_json::from_str::<JournalLine>(line) else {
                continue;
            };
            match parsed.kind {
                JournalLineKind::Header => header = parsed.header,
                JournalLineKind::Record => {
                    if let Some(rec) = parsed.record {
                        records.push(rec);
                    }
                }
                JournalLineKind::Footer => has_footer = true,
            }
        }
        if has_footer {
            continue;
        }
        let Some(header) = header else {
            continue;
        };
        let age = Utc::now().signed_duration_since(header.started_at);
        if age < chrono::Duration::seconds(60) {
            continue;
        }
        if !header.atomic {
            append_footer_to_path(&path, JournalStatus::Committed, true);
            continue;
        }
        let mut restored = 0usize;
        let mut failed = 0usize;
        for record in records.iter().rev() {
            if let Some(pre) = &record.pre_image {
                let current = std::fs::read(&pre.path).ok();
                let current_sha = current.as_ref().map(|b| sha256_hex(b));
                if current_sha == pre.sha256 {
                    continue;
                }
                if let Some(bytes) = current.as_ref() {
                    let backup = path.with_extension(format!("op{}.recovered", record.op_index));
                    let _ = std::fs::write(&backup, bytes);
                }
                match restore_one(pre) {
                    Ok(()) => restored += 1,
                    Err(e) => {
                        failed += 1;
                        tracing::error!(
                            target: "apply_model.ops_applier",
                            path = %pre.path.display(),
                            error = %e,
                            "pending journal recovery failed to restore a pre-image"
                        );
                    }
                }
            } else if let EditOp::RenameFile { from, to, .. } = &record.op {
                let _ = std::fs::rename(to, from);
            }
        }
        append_footer_to_path(&path, JournalStatus::RolledBack, failed > 0);
        tracing::warn!(
            target: "apply_model.ops_applier",
            batch_id = %header.batch_id,
            restored,
            failed,
            journal = %path.display(),
            "recovered torn atomic edit batch from pending journal; \
             overwritten torn content saved alongside the journal"
        );
    }
}

fn journal_is_finalized(path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return true;
    };
    for line in raw.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        return serde_json::from_str::<JournalLine>(line)
            .map(|l| matches!(l.kind, JournalLineKind::Footer))
            .unwrap_or(true);
    }
    true
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn stamp_post_images_in_journal(path: &Path, post_images: &[(usize, String)]) {
    let Ok(existing) = std::fs::read_to_string(path) else {
        return;
    };
    let lookup: std::collections::HashMap<usize, &str> = post_images
        .iter()
        .map(|(idx, sha)| (*idx, sha.as_str()))
        .collect();
    let mut out = String::with_capacity(existing.len());
    for line in existing.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<JournalLine>(line) {
            Ok(mut parsed) => {
                if let JournalLineKind::Record = parsed.kind {
                    if let Some(rec) = parsed.record.as_mut() {
                        if let Some(sha) = lookup.get(&rec.op_index) {
                            rec.post_image_sha256 = Some((*sha).to_string());
                        }
                    }
                }
                match serde_json::to_string(&parsed) {
                    Ok(rewritten) => {
                        out.push_str(&rewritten);
                        out.push('\n');
                    }
                    Err(_) => {
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            }
            Err(_) => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    let _ = atomic_write(path, out.as_bytes());
}

fn build_named_scopes_for(
    path: &Path,
    anchor: &crate::apply_model::edit_op::ScopeAnchor,
) -> Vec<crate::apply_model::heuristic::NamedScope> {
    use crate::apply_model::heuristic::NamedScope;
    use std::ops::Range;

    let kind = scope_kind_from_str(&anchor.kind);
    if let Some(byte_range) = anchor.byte_range.clone() {
        let Ok(src) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        let clamped = byte_range.start.min(src.len())..byte_range.end.min(src.len());
        if clamped.start >= clamped.end {
            return Vec::new();
        }
        let line_range = byte_range_to_line_range(&src, &clamped);
        return vec![NamedScope {
            kind,
            name: anchor.name.clone(),
            byte_range: clamped,
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

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    crate::util::atomic_write(path, bytes)
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

fn byte_range_to_line_range(
    source: &str,
    byte_range: &std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    let start = byte_range.start.min(source.len());
    let end = byte_range.end.min(source.len());
    let start_line = source[..start].bytes().filter(|b| *b == b'\n').count() + 1;
    let mut end_line = source[..end].bytes().filter(|b| *b == b'\n').count() + 1;
    if end > 0 && source.as_bytes()[end - 1] == b'\n' {
        end_line = end_line.saturating_sub(1);
    }
    start_line..end_line.max(start_line)
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

fn first_conflicting_multi_op_path(batch: &EditBatch) -> Option<PathBuf> {
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for op in &batch.ops {
        let path = match op {
            EditOp::Replace { path, .. }
            | EditOp::Insert { path, .. }
            | EditOp::Delete { path, .. } => path.clone(),
            _ => continue,
        };
        if !seen.insert(path.clone()) {
            return Some(path);
        }
    }
    None
}

fn region_requests_for_batch(batch: &EditBatch) -> Vec<RegionLockRequest> {
    let mut out: Vec<RegionLockRequest> = Vec::with_capacity(batch.ops.len());
    for op in &batch.ops {
        match op {
            EditOp::Replace { path, .. }
            | EditOp::Insert { path, .. }
            | EditOp::Delete { path, .. } => out.push(RegionLockRequest {
                path: path.clone(),
                range: 0..usize::MAX,
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

            let (rename_from, rename_from_bytes) = if let EditOp::RenameFile { from, .. } = op {
                (Some(from.clone()), std::fs::read(from).ok())
            } else {
                (None, None)
            };
            map.insert(
                path.clone(),
                PreImage {
                    path,
                    bytes,
                    rename_target_bytes: None,
                    rename_from,
                    rename_from_bytes,
                    sha256,
                    mtime_ms,
                },
            );
        }
    }
    Ok(map)
}

fn rotate_journals_in(dir: &Path, retention: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
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
    if files.len() <= retention {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    let mut to_remove = files.len() - retention;
    const STALE_PENDING_MAX_AGE: std::time::Duration =
        std::time::Duration::from_secs(7 * 24 * 60 * 60);
    for (mtime, path) in files.into_iter() {
        if to_remove == 0 {
            break;
        }
        if !journal_is_finalized(&path) {
            let stale = mtime
                .elapsed()
                .map(|age| age > STALE_PENDING_MAX_AGE)
                .unwrap_or(false);
            if !stale {
                continue;
            }
        }
        let _ = std::fs::remove_file(path);
        to_remove -= 1;
    }
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
            let _ = atomic_write(path, existing.as_bytes());
        }
    }
}

fn restore_pre_images(map: &BTreeMap<PathBuf, PreImage>) -> Result<(), std::io::Error> {

    for pre in map.values() {
        restore_one(pre)?;
    }
    Ok(())
}

async fn restore_pre_images_async(
    map: &BTreeMap<PathBuf, PreImage>,
) -> Result<(), std::io::Error> {
    let cloned = map.clone();
    match tokio::task::spawn_blocking(move || restore_pre_images(&cloned)).await {
        Ok(result) => result,
        Err(join_err) => Err(std::io::Error::other(format!(
            "rollback task failed to join: {join_err}"
        ))),
    }
}

fn restore_path_bytes(path: &Path, bytes: &Option<Vec<u8>>) -> Result<(), std::io::Error> {
    match bytes {
        Some(bytes) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomic_write(path, bytes)
        }
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        },
    }
}

fn post_image_is_fresh(path: &Path, expected_sha256: Option<&str>) -> bool {
    let Some(expected) = expected_sha256 else {
        return true;
    };
    match std::fs::read(path) {
        Ok(bytes) => sha256_hex(&bytes) == expected,
        Err(_) => true,
    }
}

fn restore_one(pre: &PreImage) -> Result<(), std::io::Error> {
    if let Some(from) = &pre.rename_from {
        restore_path_bytes(from, &pre.rename_from_bytes)?;
    }
    restore_path_bytes(&pre.path, &pre.bytes)
}

fn render_minimal_unified_diff(path: &Path, before: &str, after: &str) -> String {
    const MAX_DIFF_INPUT_BYTES: usize = 2 * 1024 * 1024;
    if before.len() + after.len() > MAX_DIFF_INPUT_BYTES {
        return format!(
            "--- a/{p}\n+++ b/{p}\n@@ diff omitted: content too large for preview ({} -> {} bytes) @@\n",
            before.len(),
            after.len(),
            p = path.display()
        );
    }
    let diff = similar::TextDiff::from_lines(before, after);
    diff.unified_diff()
        .context_radius(3)
        .header(
            &format!("a/{}", path.display()),
            &format!("b/{}", path.display()),
        )
        .to_string()
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
