// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::reference_helpers::{self as helpers, RefKind};
use super::state::CuratorState;
use super::tools::{curators_base_dir, ensure_inside_curator};
use crate::security::SecurityPolicy;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct CuratorLocalReferenceTool {
    state: CuratorState,
    security: Arc<SecurityPolicy>,
}

impl CuratorLocalReferenceTool {
    pub fn new(state: CuratorState, security: Arc<SecurityPolicy>) -> Self {
        Self { state, security }
    }
}

#[async_trait]
impl Tool for CuratorLocalReferenceTool {
    fn name(&self) -> &str {
        "curator_local_reference"
    }

    fn description(&self) -> &str {
        "Register one or more LOCAL reference projects (already present inside the workspace) \
         as Curator references. For each project the tool extracts README / LICENSE / AGENTS.md \
         / ARCHITECTURE.md / build manifests and a key-source skeleton, then appends a `[Ln]` \
         entry to `sources.md` and a structured section to `research_notes.md`. Use this for \
         git submodules, vendored libraries, sister projects, or third-party reference codebases \
         the user has manually placed under the current workspace. Paths must resolve INSIDE \
         the workspace and OUTSIDE `.senweavercoding/curators/` (the curator output area)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "projects": {
                    "type": "array",
                    "description": "List of local reference projects already placed under the workspace. Each entry may be a workspace-relative path string or an object with path and optional subpath, label, note.",
                    "items": {
                        "oneOf": [
                            {"type": "string"},
                            {
                                "type": "object",
                                "properties": {
                                    "path": {"type": "string", "description": "Workspace-relative directory path. Required."},
                                    "subpath": {"type": "string", "description": "Optional subdirectory inside <path> to focus the scan on."},
                                    "label": {"type": "string", "description": "Optional human-readable label (defaults to the trailing directory name)."},
                                    "note": {"type": "string", "description": "Optional one-line context note (why this project is being added)."}
                                },
                                "required": ["path"]
                            }
                        ]
                    },
                    "minItems": 1
                },
                "max_files_per_project": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 30,
                    "description": "How many source files to include in each project's skeleton (default 10)."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags to attach to every appended source/note entry."
                }
            },
            "required": ["projects"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let active = self
            .state
            .get()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "curator_local_reference requires an active Curator session (call enter_curator_mode first)."
                )
            })?;
        ensure_inside_curator(&active.root_dir, &self.security)?;

        let entries = parse_local_entries(args.get("projects"))?;
        if entries.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("curator_local_reference requires a non-empty 'projects' array".into()),
            });
        }
        let max_files = args
            .get("max_files_per_project")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(10)
            .clamp(1, 30);
        let tags = args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let workspace = self.security.workspace_dir();
        let workspace_abs =
            std::fs::canonicalize(&workspace).unwrap_or_else(|_| workspace.clone());
        let curators_base = curators_base_dir(&workspace);
        let curators_base_abs =
            std::fs::canonicalize(&curators_base).unwrap_or(curators_base.clone());

        let mut success_summaries: Vec<String> = Vec::new();
        let mut failure_summaries: Vec<String> = Vec::new();
        let mut total_appended_bytes: usize = 0;

        for entry in entries {
            let workspace_abs = workspace_abs.clone();
            let curators_base_abs = curators_base_abs.clone();
            let root_dir = active.root_dir.clone();
            let tags = tags.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                process_local_entry(entry, &workspace_abs, &curators_base_abs, &root_dir, max_files, &tags)
            })
            .await
            .unwrap_or_else(|e| {
                Ok(LocalRefOutcome::Failure(format!(
                    "curator_local_reference: internal task error: {e}"
                )))
            })?;
            match outcome {
                LocalRefOutcome::Success {
                    summary,
                    appended_bytes,
                    sources_path,
                    sources_before,
                    sources_after,
                    notes_path,
                    notes_before,
                    notes_after,
                } => {
                    if let Some(after) = sources_after.as_deref() {
                        crate::session::record_write_for_current_session(&sources_path);
                        crate::agent::file_edit_emitter::emit_file_edit(
                            &sources_path,
                            sources_before.as_deref(),
                            Some(after),
                            None,
                        )
                        .await;
                    }
                    if let Some(after) = notes_after.as_deref() {
                        crate::session::record_write_for_current_session(&notes_path);
                        crate::agent::file_edit_emitter::emit_file_edit(
                            &notes_path,
                            notes_before.as_deref(),
                            Some(after),
                            None,
                        )
                        .await;
                    }
                    total_appended_bytes += appended_bytes;
                    success_summaries.push(summary);
                }
                LocalRefOutcome::Failure(msg) => failure_summaries.push(msg),
            }
        }

        let success_count = success_summaries.len();
        let mut output = format!(
            "curator_local_reference processed {} request(s): {} success, {} failure. \
             Appended {total_appended_bytes} bytes across sources.md + research_notes.md.\n",
            success_count + failure_summaries.len(),
            success_count,
            failure_summaries.len()
        );
        if !success_summaries.is_empty() {
            output.push_str("Successes:\n");
            output.push_str(&success_summaries.join("\n"));
            output.push('\n');
        }
        if !failure_summaries.is_empty() {
            output.push_str("\nFailures:\n");
            for s in &failure_summaries {
                output.push_str(&format!("  ✗ {s}\n"));
            }
        }
        let ok = success_count > 0;
        Ok(ToolResult {
            success: ok,
            output,
            error: if ok {
                None
            } else {
                Some("curator_local_reference: every requested project failed".to_string())
            },
        })
    }
}

enum LocalRefOutcome {
    Success {
        summary: String,
        appended_bytes: usize,
        sources_path: PathBuf,
        sources_before: Option<Vec<u8>>,
        sources_after: Option<Vec<u8>>,
        notes_path: PathBuf,
        notes_before: Option<Vec<u8>>,
        notes_after: Option<Vec<u8>>,
    },
    Failure(String),
}

fn process_local_entry(
    entry: LocalEntry,
    workspace_abs: &Path,
    curators_base_abs: &Path,
    root_dir: &Path,
    max_files: usize,
    tags: &[String],
) -> anyhow::Result<LocalRefOutcome> {
    let resolved = match resolve_local_path(&entry.path, workspace_abs) {
        Ok(p) => p,
        Err(e) => return Ok(LocalRefOutcome::Failure(format!("{}  -  {e}", entry.path))),
    };
    if resolved.starts_with(curators_base_abs) {
        return Ok(LocalRefOutcome::Failure(format!(
            "{}  -  path is inside `.senweavercoding/curators/`; curator output cannot be its own reference",
            entry.path
        )));
    }
    if !resolved.is_dir() {
        return Ok(LocalRefOutcome::Failure(format!(
            "{}  -  resolved path `{}` is not a directory",
            entry.path,
            resolved.display()
        )));
    }
    let title_label = entry
        .label
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| derive_label(&resolved));
    let rel_path = pathdiff_or_self(&resolved, workspace_abs);

    let metadata = helpers::detect_repo_metadata(&resolved, entry.subpath.as_deref());
    let skeleton = helpers::scan_code_skeleton(&resolved, entry.subpath.as_deref(), max_files);

    let captured_at = helpers::iso_now();
    let id = helpers::next_ref_id(root_dir, RefKind::Local)?;
    let mut extras: Vec<(&'static str, String)> = Vec::new();
    extras.push(("Workspace path", rel_path.clone()));
    extras.push(("Absolute path", resolved.to_string_lossy().to_string()));
    if let Some(license) = &metadata.license_name {
        extras.push(("License", license.clone()));
    }
    if let Some(sub) = &entry.subpath {
        extras.push(("Focused subpath", sub.clone()));
    }

    let source_entry = helpers::render_source_entry_for_reference(
        &id,
        &title_label,
        "Workspace path",
        &rel_path,
        "local reference project",
        &extras,
        &captured_at,
        if tags.is_empty() { None } else { Some(tags) },
        entry.note.as_deref(),
    );

    let notes_entry = helpers::render_research_notes_for_reference(
        &id,
        &title_label,
        "local reference project",
        "Workspace path",
        &rel_path,
        &metadata,
        &skeleton,
        &captured_at,
        entry.note.as_deref(),
    );

    let sources_path = helpers::sources_path(root_dir);
    let notes_path = helpers::notes_path(root_dir);

    let sources_before = std::fs::read(&sources_path).ok();
    helpers::append_file(&sources_path, &source_entry)?;
    let sources_after = std::fs::read(&sources_path).ok();
    let notes_before = std::fs::read(&notes_path).ok();
    helpers::append_file(&notes_path, &notes_entry)?;
    let notes_after = std::fs::read(&notes_path).ok();

    let summary = format!(
        "  ✓ {id} {}  -  {} ({} key files; license={})",
        rel_path,
        title_label,
        skeleton.len(),
        metadata
            .license_name
            .clone()
            .unwrap_or_else(|| "unknown".into())
    );

    Ok(LocalRefOutcome::Success {
        summary,
        appended_bytes: source_entry.len() + notes_entry.len(),
        sources_path,
        sources_before,
        sources_after,
        notes_path,
        notes_before,
        notes_after,
    })
}

#[derive(Debug, Clone)]
struct LocalEntry {
    path: String,
    subpath: Option<String>,
    label: Option<String>,
    note: Option<String>,
}

fn parse_local_entries(raw: Option<&Value>) -> anyhow::Result<Vec<LocalEntry>> {
    let arr = raw
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("curator_local_reference: 'projects' must be an array"))?;
    let mut out: Vec<LocalEntry> = Vec::new();
    for item in arr {
        match item {
            Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    continue;
                }
                out.push(LocalEntry {
                    path: trimmed.to_string(),
                    subpath: None,
                    label: None,
                    note: None,
                });
            }
            Value::Object(obj) => {
                let path = obj
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "curator_local_reference: object entry missing required 'path' string"
                        )
                    })?
                    .to_string();
                out.push(LocalEntry {
                    path,
                    subpath: obj
                        .get("subpath")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().trim_matches('/').to_string())
                        .filter(|s| !s.is_empty()),
                    label: obj
                        .get("label")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                    note: obj
                        .get("note")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                });
            }
            _ => continue,
        }
    }
    Ok(out)
}

fn resolve_local_path(raw: &str, workspace_abs: &Path) -> Result<PathBuf, String> {
    let raw_path = Path::new(raw);
    if raw_path.is_absolute() {
        return Err(
            "absolute paths are rejected  -  pass a workspace-relative path".to_string(),
        );
    }
    let joined = workspace_abs.join(raw_path);
    let canonical = std::fs::canonicalize(&joined)
        .map_err(|e| format!("cannot resolve `{}`: {e}", joined.display()))?;
    if !canonical.starts_with(workspace_abs) {
        return Err(format!(
            "resolved path `{}` escapes the workspace `{}`",
            canonical.display(),
            workspace_abs.display()
        ));
    }
    Ok(canonical)
}

fn derive_label(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn pathdiff_or_self(target: &Path, base: &Path) -> String {
    target
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| target.to_string_lossy().to_string().replace('\\', "/"))
}
