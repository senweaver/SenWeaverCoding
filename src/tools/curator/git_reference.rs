// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::reference_helpers::{self as helpers, ParsedGitUrl, REFS_GIT_SUBDIR, RefKind};
use super::state::CuratorState;
use super::tools::ensure_inside_curator;
use crate::security::SecurityPolicy;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

const GIT_TIMEOUT: Duration = Duration::from_secs(180);

pub struct CuratorGitReferenceTool {
    state: CuratorState,
    security: Arc<SecurityPolicy>,
}

impl CuratorGitReferenceTool {
    pub fn new(state: CuratorState, security: Arc<SecurityPolicy>) -> Self {
        Self { state, security }
    }
}

#[async_trait]
impl Tool for CuratorGitReferenceTool {
    fn name(&self) -> &str {
        "curator_git_reference"
    }

    fn description(&self) -> &str {
        "Add one or more remote git repositories as Curator reference projects. Each repo is \
         shallow-cloned (depth=1, blob filter) into \
         `.senweavercoding/curators/<slug>/refs/git/<host>__<owner>__<repo>/`, then scanned for \
         README / LICENSE / AGENTS.md / ARCHITECTURE.md / build manifests (Cargo.toml, \
         package.json, go.mod, pyproject.toml, …) and the largest in-scope source files. \
         Each repo produces a `[Gn]` entry in `sources.md` (with origin URL, commit SHA, \
         license, local path, optional user note) and a structured section in \
         `research_notes.md` (README excerpt, architecture excerpt, build-manifest heads, \
         key source skeletons). Re-running on the same URL reuses the cached clone."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "repos": {
                    "type": "array",
                    "description": "List of git repos to add as reference material. Each entry can be a clone URL string or an object with url, optional ref, subpath, label, note.",
                    "items": {
                        "oneOf": [
                            {"type": "string"},
                            {
                                "type": "object",
                                "properties": {
                                    "url": {"type": "string", "description": "Clone URL (HTTPS or SSH). Required when entry is an object."},
                                    "ref": {"type": "string", "description": "Optional branch / tag / SHA to checkout (shallow). Defaults to the remote's HEAD."},
                                    "subpath": {"type": "string", "description": "Optional repo-relative subdirectory to focus the metadata + skeleton scan on."},
                                    "label": {"type": "string", "description": "Optional human-readable label (defaults to `<owner>/<repo>`)."},
                                    "note": {"type": "string", "description": "Optional one-line context note (why this repo is being added)."}
                                },
                                "required": ["url"]
                            }
                        ]
                    },
                    "minItems": 1
                },
                "max_files_per_repo": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 30,
                    "description": "How many source files to include in each repo's skeleton (default 10)."
                },
                "clone_depth": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "description": "git clone --depth value (default 1)."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags to attach to every appended source/note entry."
                },
                "refresh": {
                    "type": "boolean",
                    "description": "If true, remove the cached clone and re-clone every repo. Default false."
                }
            },
            "required": ["repos"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let active = self
            .state
            .read()
            .clone()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "curator_git_reference requires an active Curator session (call enter_curator_mode first)."
                )
            })?;
        ensure_inside_curator(&active.root_dir, &self.security)?;

        let repo_entries = parse_repo_entries(args.get("repos"))?;
        if repo_entries.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("curator_git_reference requires a non-empty 'repos' array".into()),
            });
        }
        let max_files = args
            .get("max_files_per_repo")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(10)
            .clamp(1, 30);
        let clone_depth = args
            .get("clone_depth")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(1)
            .clamp(1, 50);
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
        let refresh = args.get("refresh").and_then(|v| v.as_bool()).unwrap_or(false);

        let refs_root = active.root_dir.join(REFS_GIT_SUBDIR);
        {
            let refs_root = refs_root.clone();
            tokio::task::spawn_blocking(move || {
                std::fs::create_dir_all(&refs_root).ok();
            })
            .await
            .ok();
        }

        let mut success_summaries: Vec<String> = Vec::new();
        let mut failure_summaries: Vec<String> = Vec::new();
        let mut total_appended_bytes: usize = 0;

        for entry in repo_entries {
            let parsed = match helpers::parse_git_url(&entry.url) {
                Some(p) => p,
                None => {
                    failure_summaries.push(format!(
                        "{}  -  cannot parse as a git URL (expected https:// or git@host:owner/repo form)",
                        entry.url
                    ));
                    continue;
                }
            };
            let slug = parsed.slug();
            let target_dir = refs_root.join(&slug);

            let clone_outcome = ensure_clone(
                &parsed,
                &target_dir,
                entry.git_ref.as_deref(),
                clone_depth,
                refresh,
            )
            .await;
            let (clone_status, clone_message) = match clone_outcome {
                Ok(status) => status,
                Err(e) => {
                    failure_summaries.push(format!("{}  -  clone failed: {e}", parsed.pretty()));
                    continue;
                }
            };

            let commit_sha = resolve_commit_sha(&target_dir).await.ok();

            let workspace = self.security.workspace_dir();
            let root_dir = active.root_dir.clone();
            let tags = tags.clone();
            let origin_url = parsed.original.clone();
            let pretty = parsed.pretty();
            let subpath = entry.subpath.clone();
            let label = entry.label.clone();
            let git_ref = entry.git_ref.clone();
            let note = entry.note.clone();
            let target_dir_owned = target_dir.clone();
            let commit_sha_owned = commit_sha.clone();
            let persisted = tokio::task::spawn_blocking(move || {
                persist_git_reference(GitRefInput {
                    target_dir: target_dir_owned,
                    workspace,
                    root_dir,
                    origin_url,
                    pretty,
                    subpath,
                    label,
                    git_ref,
                    note,
                    commit_sha: commit_sha_owned,
                    clone_status,
                    clone_message,
                    max_files,
                    tags,
                })
            })
            .await
            .map_err(|e| anyhow::anyhow!("curator_git_reference: internal task error: {e}"))??;

            if let Some(after) = persisted.sources_after.as_deref() {
                crate::agent::file_edit_emitter::emit_file_edit(
                    &persisted.sources_path,
                    persisted.sources_before.as_deref(),
                    Some(after),
                    None,
                )
                .await;
            }
            if let Some(after) = persisted.notes_after.as_deref() {
                crate::agent::file_edit_emitter::emit_file_edit(
                    &persisted.notes_path,
                    persisted.notes_before.as_deref(),
                    Some(after),
                    None,
                )
                .await;
            }

            total_appended_bytes += persisted.appended_bytes;
            success_summaries.push(persisted.summary);
        }

        let success_count = success_summaries.len();
        let mut output = format!(
            "curator_git_reference processed {} request(s): {} success, {} failure. \
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
                Some("curator_git_reference: every requested repository failed".to_string())
            },
        })
    }
}

#[derive(Debug, Clone)]
struct RepoEntry {
    url: String,
    git_ref: Option<String>,
    subpath: Option<String>,
    label: Option<String>,
    note: Option<String>,
}

fn parse_repo_entries(raw: Option<&Value>) -> anyhow::Result<Vec<RepoEntry>> {
    let arr = raw
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("curator_git_reference: 'repos' must be an array"))?;
    let mut out: Vec<RepoEntry> = Vec::new();
    for item in arr {
        match item {
            Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    continue;
                }
                out.push(RepoEntry {
                    url: trimmed.to_string(),
                    git_ref: None,
                    subpath: None,
                    label: None,
                    note: None,
                });
            }
            Value::Object(obj) => {
                let url = obj
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("curator_git_reference: object entry missing required 'url' string")
                    })?
                    .to_string();
                out.push(RepoEntry {
                    url,
                    git_ref: obj
                        .get("ref")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
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

struct GitRefInput {
    target_dir: std::path::PathBuf,
    workspace: std::path::PathBuf,
    root_dir: std::path::PathBuf,
    origin_url: String,
    pretty: String,
    subpath: Option<String>,
    label: Option<String>,
    git_ref: Option<String>,
    note: Option<String>,
    commit_sha: Option<String>,
    clone_status: CloneStatus,
    clone_message: String,
    max_files: usize,
    tags: Vec<String>,
}

struct GitPersistOutcome {
    summary: String,
    appended_bytes: usize,
    sources_path: std::path::PathBuf,
    sources_before: Option<Vec<u8>>,
    sources_after: Option<Vec<u8>>,
    notes_path: std::path::PathBuf,
    notes_before: Option<Vec<u8>>,
    notes_after: Option<Vec<u8>>,
}

fn persist_git_reference(input: GitRefInput) -> anyhow::Result<GitPersistOutcome> {
    let metadata = helpers::detect_repo_metadata(&input.target_dir, input.subpath.as_deref());
    let skeleton =
        helpers::scan_code_skeleton(&input.target_dir, input.subpath.as_deref(), input.max_files);

    let local_rel_path = pathdiff_or_self(&input.target_dir, &input.workspace);
    let title_label = input
        .label
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| input.pretty.clone());

    let captured_at = helpers::iso_now();
    let id = helpers::next_ref_id(&input.root_dir, RefKind::Git)?;
    let mut extras: Vec<(&'static str, String)> = Vec::new();
    extras.push(("Origin", input.origin_url.clone()));
    extras.push(("Host / Owner / Repo", input.pretty.clone()));
    if let Some(r) = &input.git_ref {
        extras.push(("Ref requested", r.clone()));
    }
    if let Some(sha) = &input.commit_sha {
        extras.push(("Commit SHA", sha.clone()));
    }
    if let Some(license) = &metadata.license_name {
        extras.push(("License", license.clone()));
    }
    extras.push(("Local cache", local_rel_path.clone()));
    extras.push((
        "Clone outcome",
        format!(
            "{}  -  {}",
            clone_status_label(input.clone_status),
            input.clone_message
        ),
    ));
    if let Some(sub) = &input.subpath {
        extras.push(("Focused subpath", sub.clone()));
    }

    let source_entry = helpers::render_source_entry_for_reference(
        &id,
        &title_label,
        "Origin URL",
        &input.origin_url,
        "git reference repository",
        &extras,
        &captured_at,
        if input.tags.is_empty() {
            None
        } else {
            Some(&input.tags)
        },
        input.note.as_deref(),
    );

    let notes_entry = helpers::render_research_notes_for_reference(
        &id,
        &title_label,
        "git reference repository",
        "Local cache",
        &local_rel_path,
        &metadata,
        &skeleton,
        &captured_at,
        input.note.as_deref(),
    );

    let sources_path = helpers::sources_path(&input.root_dir);
    let notes_path = helpers::notes_path(&input.root_dir);
    let sources_before = std::fs::read(&sources_path).ok();
    helpers::append_file(&sources_path, &source_entry)?;
    let sources_after = std::fs::read(&sources_path).ok();
    let notes_before = std::fs::read(&notes_path).ok();
    helpers::append_file(&notes_path, &notes_entry)?;
    let notes_after = std::fs::read(&notes_path).ok();

    let summary = format!(
        "  ✓ {id} {}  -  {} ({} key files; license={}; sha={}; status={})",
        input.pretty,
        title_label,
        skeleton.len(),
        metadata
            .license_name
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        input.commit_sha.clone().unwrap_or_else(|| "?".into()),
        clone_status_label(input.clone_status)
    );

    Ok(GitPersistOutcome {
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

#[derive(Debug, Clone, Copy)]
enum CloneStatus {
    Cloned,
    Cached,
    Refreshed,
}

fn clone_status_label(status: CloneStatus) -> &'static str {
    match status {
        CloneStatus::Cloned => "cloned",
        CloneStatus::Cached => "cached",
        CloneStatus::Refreshed => "refreshed",
    }
}

async fn ensure_clone(
    parsed: &ParsedGitUrl,
    target_dir: &Path,
    git_ref: Option<&str>,
    depth: usize,
    refresh: bool,
) -> anyhow::Result<(CloneStatus, String)> {
    let already = target_dir.join(".git").is_dir();
    if already && !refresh {
        return Ok((CloneStatus::Cached, format!("reused {}", target_dir.display())));
    }
    {
        let target_dir = target_dir.to_path_buf();
        let parent = target_dir.parent().map(Path::to_path_buf);
        let do_refresh = already && refresh;
        tokio::task::spawn_blocking(move || {
            if do_refresh {
                let _ = std::fs::remove_dir_all(&target_dir);
            }
            if let Some(parent) = parent {
                std::fs::create_dir_all(parent).ok();
            }
        })
        .await
        .ok();
    }

    let mut args: Vec<String> = vec![
        "clone".into(),
        "--no-tags".into(),
        "--single-branch".into(),
        "--quiet".into(),
        format!("--depth={depth}"),
        "--filter=blob:none".into(),
    ];
    if let Some(r) = git_ref {
        args.push("--branch".into());
        args.push(r.to_string());
    }
    args.push(parsed.original.clone());
    args.push(target_dir.to_string_lossy().to_string());

    let mut cmd: Command = crate::util::hidden_async_command("git");
    cmd.args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never");

    let started = std::time::Instant::now();
    let output = match tokio::time::timeout(GIT_TIMEOUT, cmd.output()).await {
        Ok(r) => r.map_err(|e| anyhow::anyhow!("spawn git clone failed: {e}"))?,
        Err(_) => {
            anyhow::bail!(
                "git clone timed out after {}s",
                GIT_TIMEOUT.as_secs()
            );
        }
    };
    let elapsed_ms = started.elapsed().as_millis();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let cleaned = stderr.lines().take(6).collect::<Vec<_>>().join(" | ");
        anyhow::bail!(
            "git clone exit {}: {}",
            output.status.code().unwrap_or(-1),
            if cleaned.is_empty() { "unknown error".into() } else { cleaned }
        );
    }
    let status = if refresh { CloneStatus::Refreshed } else { CloneStatus::Cloned };
    Ok((status, format!("ok in {elapsed_ms} ms")))
}

async fn resolve_commit_sha(target_dir: &Path) -> anyhow::Result<String> {
    let mut cmd: Command = crate::util::hidden_async_command("git");
    cmd.args(["rev-parse", "HEAD"])
        .current_dir(target_dir);
    let output = tokio::time::timeout(Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("git rev-parse timed out"))?
        .map_err(|e| anyhow::anyhow!("spawn git rev-parse failed: {e}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn pathdiff_or_self(target: &Path, base: &Path) -> String {
    target
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| target.to_string_lossy().to_string().replace('\\', "/"))
}
