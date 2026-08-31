// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tools::error::ToolErrorCause;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Function,
    Class,
    Module,
    Block,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditAnchor {
    pub kind: ScopeKind,
    pub name: String,
    #[serde(with = "range_serde")]
    pub byte_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScopeAnchor {

    pub kind: String,

    pub name: String,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_range_serde"
    )]
    pub byte_range: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotebookCellOp {
    Replace {
        cell_index: usize,
        new_source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell_type: Option<String>,
    },
    Insert {
        cell_index: usize,
        new_source: String,
        cell_type: String,
        #[serde(default)]
        insert_before: bool,
    },
    Delete {
        cell_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {

    Replace {
        path: PathBuf,
        #[serde(with = "range_serde")]
        byte_range: Range<usize>,
        old_text: String,
        new_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<EditAnchor>,
    },

    Insert {
        path: PathBuf,
        at_byte: usize,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<EditAnchor>,
    },

    Delete {
        path: PathBuf,
        #[serde(with = "range_serde")]
        byte_range: Range<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        old_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<EditAnchor>,
    },

    CreateFile {
        path: PathBuf,
        contents: String,
        overwrite: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_pre_sha256: Option<String>,
    },

    DeleteFile {
        path: PathBuf,
        missing_ok: bool,
    },

    RenameFile {
        from: PathBuf,
        to: PathBuf,
        overwrite: bool,
    },

    ApplyHunk {
        path: PathBuf,
        diff: String,
        fuzz: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope_anchor: Option<ScopeAnchor>,
    },

    NotebookCell {
        path: PathBuf,
        cell: NotebookCellOp,
    },
}

impl EditOp {

    #[must_use]
    pub fn primary_path(&self) -> &Path {
        match self {
            EditOp::Replace { path, .. }
            | EditOp::Insert { path, .. }
            | EditOp::Delete { path, .. }
            | EditOp::CreateFile { path, .. }
            | EditOp::DeleteFile { path, .. }
            | EditOp::ApplyHunk { path, .. }
            | EditOp::NotebookCell { path, .. } => path,
            EditOp::RenameFile { to, .. } => to,
        }
    }

    #[must_use]
    pub fn touched_paths(&self) -> Vec<&Path> {
        match self {
            EditOp::RenameFile { from, to, .. } => vec![from.as_path(), to.as_path()],
            other => vec![other.primary_path()],
        }
    }

    pub fn validate_preconditions(
        &self,
        workspace_root: &Path,
    ) -> Result<(), PreconditionError> {
        self.validate_preconditions_with_roots(workspace_root, &[])
    }

    pub fn validate_preconditions_with_roots(
        &self,
        workspace_root: &Path,
        allowed_roots: &[PathBuf],
    ) -> Result<(), PreconditionError> {
        for path in self.touched_paths() {
            ensure_inside_any(workspace_root, allowed_roots, path)?;
        }

        match self {
            EditOp::Replace {
                path,
                byte_range,
                old_text,
                ..
            } => {
                let bytes = read_bytes(path)?;
                ensure_range_in_bounds(path, byte_range, bytes.len())?;
                let actual = &bytes[byte_range.clone()];
                if actual != old_text.as_bytes() {
                    return Err(PreconditionError::ContentMismatch {
                        path: path.clone(),
                        byte_range: byte_range.clone(),
                    });
                }
                Ok(())
            }
            EditOp::Insert { path, at_byte, .. } => {
                let bytes = read_bytes(path)?;
                if *at_byte > bytes.len() {
                    return Err(PreconditionError::OutOfBounds {
                        path: path.clone(),
                        offset: *at_byte,
                        len: bytes.len(),
                    });
                }
                Ok(())
            }
            EditOp::Delete {
                path,
                byte_range,
                old_text,
                ..
            } => {
                let bytes = read_bytes(path)?;
                ensure_range_in_bounds(path, byte_range, bytes.len())?;
                if let Some(expected) = old_text {
                    if &bytes[byte_range.clone()] != expected.as_bytes() {
                        return Err(PreconditionError::ContentMismatch {
                            path: path.clone(),
                            byte_range: byte_range.clone(),
                        });
                    }
                }
                Ok(())
            }
            EditOp::CreateFile {
                path,
                overwrite,
                expected_pre_sha256,
                ..
            } => {
                if !overwrite && path.exists() {
                    return Err(PreconditionError::FileExists { path: path.clone() });
                }
                if *overwrite {
                    if let Some(expected) = expected_pre_sha256 {
                        if path.exists() {
                            let bytes = read_bytes(path)?;
                            let actual = sha256_hex(&bytes);
                            if !actual.eq_ignore_ascii_case(expected) {
                                return Err(PreconditionError::StaleOverwrite {
                                    path: path.clone(),
                                });
                            }
                        } else {
                            return Err(PreconditionError::StaleOverwrite {
                                path: path.clone(),
                            });
                        }
                    }
                }
                Ok(())
            }
            EditOp::DeleteFile { path, missing_ok } => {
                if !path.exists() && !missing_ok {
                    return Err(PreconditionError::FileMissing { path: path.clone() });
                }
                Ok(())
            }
            EditOp::RenameFile {
                from,
                to,
                overwrite,
            } => {
                if !from.exists() {
                    return Err(PreconditionError::FileMissing { path: from.clone() });
                }
                if !overwrite && to.exists() {
                    return Err(PreconditionError::FileExists { path: to.clone() });
                }
                Ok(())
            }
            EditOp::ApplyHunk { path, .. } => {
                if !path.exists() {
                    return Err(PreconditionError::FileMissing { path: path.clone() });
                }
                Ok(())
            }
            EditOp::NotebookCell { path, .. } => {
                if !path.exists() {
                    return Err(PreconditionError::FileMissing { path: path.clone() });
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "origin", rename_all = "snake_case")]
pub enum EditOrigin {
    InlineEdit,
    CodeEditFlow,
    WriteMode,
    PatchTool,
    FileEditTool,
    FileWriteTool,
    MultiEditTool,
    GlobEditTool,
    XfileRefactorTool,
    LspFormatTool,
    NotebookEditTool,
    DiffSession,
    Agent { id: String },
}

impl EditOrigin {

    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            EditOrigin::InlineEdit => "inline_edit",
            EditOrigin::CodeEditFlow => "code_edit_flow",
            EditOrigin::WriteMode => "write_mode",
            EditOrigin::PatchTool => "patch_apply",
            EditOrigin::FileEditTool => "file_edit",
            EditOrigin::FileWriteTool => "file_write",
            EditOrigin::MultiEditTool => "multi_edit",
            EditOrigin::GlobEditTool => "glob_edit",
            EditOrigin::XfileRefactorTool => "code_xfile_refactor",
            EditOrigin::LspFormatTool => "lsp_format",
            EditOrigin::NotebookEditTool => "notebook_edit",
            EditOrigin::DiffSession => "diff_session",
            EditOrigin::Agent { .. } => "agent",
        }
    }

    #[must_use]
    pub fn holder_tag(&self) -> String {
        match self {
            EditOrigin::Agent { id } => format!("agent:{id}"),
            other => other.tag().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditBatch {
    pub batch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub origin: EditOrigin,
    pub ops: Vec<EditOp>,

    pub atomic: bool,
}

impl EditBatch {

    #[must_use]
    pub fn new(origin: EditOrigin) -> Self {
        Self {
            batch_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: None,
            origin,
            ops: Vec::new(),
            atomic: true,
        }
    }

    #[must_use]
    pub fn with_atomic(mut self, atomic: bool) -> Self {
        self.atomic = atomic;
        self
    }

    #[must_use]
    pub fn with_correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    #[must_use]
    pub fn with_op(mut self, op: EditOp) -> Self {
        self.ops.push(op);
        self
    }

    #[must_use]
    pub fn with_ops<I: IntoIterator<Item = EditOp>>(mut self, ops: I) -> Self {
        self.ops.extend(ops);
        self
    }

    pub fn push(&mut self, op: EditOp) {
        self.ops.push(op);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PreconditionError {
    #[error("path escapes workspace root: {path}")]
    PathEscape { path: PathBuf },
    #[error("file does not exist: {path}")]
    FileMissing { path: PathBuf },
    #[error("file already exists: {path}")]
    FileExists { path: PathBuf },
    #[error("byte range {byte_range:?} is out of bounds for {path} (len={len})")]
    RangeOutOfBounds {
        path: PathBuf,
        byte_range: Range<usize>,
        len: usize,
    },
    #[error("byte offset {offset} exceeds file length {len} in {path}")]
    OutOfBounds {
        path: PathBuf,
        offset: usize,
        len: usize,
    },
    #[error("file content at {byte_range:?} in {path} does not match expected old_text")]
    ContentMismatch {
        path: PathBuf,
        byte_range: Range<usize>,
    },
    #[error("file {path} changed on disk since it was read (full-file overwrite rejected to avoid clobbering a concurrent edit); re-read the current contents and retry")]
    StaleOverwrite { path: PathBuf },
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl From<PreconditionError> for ToolErrorCause {
    fn from(value: PreconditionError) -> Self {
        match value {
            PreconditionError::Io { source, .. } => ToolErrorCause::Io(source),
            PreconditionError::PathEscape { .. }
            | PreconditionError::FileMissing { .. }
            | PreconditionError::FileExists { .. }
            | PreconditionError::RangeOutOfBounds { .. }
            | PreconditionError::OutOfBounds { .. }
            | PreconditionError::ContentMismatch { .. }
            | PreconditionError::StaleOverwrite { .. } => {
                ToolErrorCause::PreconditionFailed(value.to_string())
            }
        }
    }
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, PreconditionError> {
    std::fs::read(path).map_err(|source| PreconditionError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn ensure_range_in_bounds(
    path: &Path,
    byte_range: &Range<usize>,
    len: usize,
) -> Result<(), PreconditionError> {
    if byte_range.start > byte_range.end || byte_range.end > len {
        return Err(PreconditionError::RangeOutOfBounds {
            path: path.to_path_buf(),
            byte_range: byte_range.clone(),
            len,
        });
    }
    Ok(())
}

fn ensure_inside_any(
    root: &Path,
    allowed_roots: &[PathBuf],
    path: &Path,
) -> Result<(), PreconditionError> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| normalize(root));
    let candidate = if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&root.join(path))
    };
    let Some(normal) = canonicalize_allowing_missing_tail(&candidate) else {
        return Err(PreconditionError::PathEscape { path: candidate });
    };

    if crate::util::path_is_within(&normal, &root) {
        return Ok(());
    }
    for extra in allowed_roots {
        let extra = std::fs::canonicalize(extra).unwrap_or_else(|_| normalize(extra));
        if crate::util::path_is_within(&normal, &extra) {
            return Ok(());
        }
    }
    Err(PreconditionError::PathEscape { path: normal })
}

fn canonicalize_allowing_missing_tail(path: &Path) -> Option<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(mut canonical) => {
                for part in suffix.iter().rev() {
                    canonical.push(part);
                }
                return Some(canonical);
            }
            Err(_) => {
                let parent = existing.parent()?;
                let name = existing.file_name()?.to_os_string();
                suffix.push(name);
                existing = parent.to_path_buf();
            }
        }
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

mod range_serde {
    use std::ops::Range;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(r: &Range<usize>, ser: S) -> Result<S::Ok, S::Error> {
        (r.start, r.end).serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Range<usize>, D::Error> {
        let (start, end) = <(usize, usize)>::deserialize(de)?;
        Ok(start..end)
    }
}

mod opt_range_serde {
    use std::ops::Range;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        r: &Option<Range<usize>>,
        ser: S,
    ) -> Result<S::Ok, S::Error> {
        match r {
            Some(r) => (r.start, r.end).serialize(ser),
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        de: D,
    ) -> Result<Option<Range<usize>>, D::Error> {
        let opt = <Option<(usize, usize)>>::deserialize(de)?;
        Ok(opt.map(|(s, e)| s..e))
    }
}
