// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameRequest {
    pub workspace_root: PathBuf,
    pub language: String,
    pub file_path: PathBuf,
    pub symbol: String,
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameEdit {
    pub file_path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameResponse {
    pub edits: Vec<RenameEdit>,
    pub source: &'static str,
    pub checkpoint_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("lsp unavailable for language {0}")]
    LspUnavailable(String),
    #[error("lsp rename failed: {0}")]
    LspFailed(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("validation failed: {0}")]
    Validation(String),
}

#[async_trait]
pub trait LspRenameClient: Send + Sync {
    async fn supports(&self, language: &str) -> bool;
    async fn rename(&self, req: &RenameRequest) -> Result<Vec<RenameEdit>, RenameError>;
    fn name(&self) -> &'static str;
}

pub fn regex_scan_edits(root: &Path, symbol: &str, new_name: &str) -> Vec<RenameEdit> {
    let mut out = Vec::new();
    let Ok(walker) = std::fs::read_dir(root) else {
        return out;
    };

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(
            ext.as_str(),
            "rs" | "py"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "go"
                | "java"
                | "cpp"
                | "cc"
                | "h"
                | "hpp"
                | "cs"
                | "rb"
                | "php"
                | "kt"
        ) {
            continue;
        }
        for (line_idx, line) in content.lines().enumerate() {
            let mut start = 0usize;
            while let Some(pos) = line[start..].find(symbol) {
                let absolute = start + pos;

                if is_word_boundary(line, absolute, symbol.len()) {
                    out.push(RenameEdit {
                        file_path: path.clone(),
                        line: line_idx as u32 + 1,
                        column: absolute as u32 + 1,
                        old_text: symbol.to_string(),
                        new_text: new_name.to_string(),
                    });
                }
                start = absolute + symbol.len();
                if start > line.len() {
                    break;
                }
            }
        }
    }
    out
}

fn is_word_boundary(line: &str, start: usize, len: usize) -> bool {
    let bytes = line.as_bytes();
    let before_ok = start == 0
        || !bytes
            .get(start - 1)
            .map(|b| is_word_byte(*b))
            .unwrap_or(false);
    let after = start + len;
    let after_ok =
        after >= bytes.len() || !bytes.get(after).map(|b| is_word_byte(*b)).unwrap_or(false);
    before_ok && after_ok
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub async fn rename_with_fallback(
    client: Option<&dyn LspRenameClient>,
    req: RenameRequest,
) -> Result<RenameResponse, RenameError> {
    let mut source: &'static str = "regex";
    let mut edits: Vec<RenameEdit> = Vec::new();

    if let Some(c) = client
        && c.supports(&req.language).await
    {
        match c.rename(&req).await {
            Ok(lsp_edits) if !lsp_edits.is_empty() => {
                edits = lsp_edits;
                source = "lsp";
                crate::observability::subsystem_metrics::incr_lsp_rename_via_lsp();
            }
            Ok(_) => {

                source = "regex";
            }
            Err(e) => {
                tracing::warn!(error = %e, "lsp rename failed; falling back to regex");
            }
        }
    }

    if edits.is_empty() {
        edits = regex_scan_edits(&req.workspace_root, &req.symbol, &req.new_name);
        source = "regex";
        if !edits.is_empty() {
            crate::observability::subsystem_metrics::incr_lsp_rename_via_regex();
        }
    }

    if edits.is_empty() {
        return Err(RenameError::Validation(format!(
            "no occurrences of symbol '{}' found",
            req.symbol
        )));
    }

    let checkpoint_id = push_rename_checkpoint(&req, &edits);

    Ok(RenameResponse {
        edits,
        source,
        checkpoint_id,
    })
}

fn push_rename_checkpoint(req: &RenameRequest, edits: &[RenameEdit]) -> Option<String> {
    use crate::agent::flows::{Artifact, Checkpoint, global_checkpoint_store};
    let store = global_checkpoint_store();
    let id = format!("rename:{}:{}", req.symbol, uuid::Uuid::new_v4());
    let body = edits
        .iter()
        .map(|e| format!("{}:{}:{}", e.file_path.display(), e.line, e.column))
        .collect::<Vec<_>>()
        .join("\n");
    let cp = Checkpoint::new(
        id.clone(),
        format!("rename({} -> {})", req.symbol, req.new_name),
        vec![Artifact::new("rename", body)],
        vec![],
    );
    store.push(cp);
    Some(id)
}
