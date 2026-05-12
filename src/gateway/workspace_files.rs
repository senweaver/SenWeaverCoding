// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Workspace file explorer API for the desktop right-sidebar.
//!
//! Endpoints (all behind bearer auth):
//!
//! - `GET    /api/workspace/tree`     — list a directory (one level by default)
//! - `GET    /api/workspace/file`     — read a file
//! - `PUT    /api/workspace/file`     — overwrite an existing file
//! - `POST   /api/workspace/file`     — create a new file
//! - `POST   /api/workspace/dir`      — create a new directory
//! - `POST   /api/workspace/move`     — rename / move
//! - `DELETE /api/workspace/entry`    — delete a file or directory
//! - `POST   /api/workspace/upload`   — upload a single file (base64 body)
//! - `GET    /api/workspace/search`   — recursively search file names
//! - `GET    /api/workspace/watch`    — SSE stream of FS change events
//!                                      (gated by the `fs-watch` cargo feature;
//!                                      returns `501 Not Implemented` otherwise
//!                                      so the route is always present)
//!
//! Path safety: every relative path is validated by [`resolve_within`]
//! which canonicalizes the requested target and asserts that it lives
//! beneath the workspace root. The `root` query parameter must be either
//! [`AppState::config.workspace_dir`] or a canonical path persisted as a
//! desktop chat session `work_dir` (see SQLite `session_metadata`), so users
//! can browse per-project folders without widening access to arbitrary disk.

use super::AppState;
use super::api::require_auth;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

const DEFAULT_TREE_DEPTH: u32 = 1;
const MAX_TREE_DEPTH: u32 = 6;
const MAX_TREE_NODES: usize = 5_000;
const MAX_FILE_READ_BYTES: u64 = 4 * 1024 * 1024;
const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 500;

const HIDDEN_DEFAULT_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".cargo",
    ".next",
    ".venv",
    "__pycache__",
    ".idea",
    ".turbo",
    ".vite",
    ".parcel-cache",
];

#[derive(Debug)]
enum FsError {
    NotFound,
    OutsideRoot,
    InvalidName,
    InvalidRoot,
    Io(std::io::Error),
    TooLarge(u64),
}

impl FsError {
    fn into_response(self) -> axum::response::Response {
        match self {
            FsError::NotFound => (StatusCode::NOT_FOUND, Json(json!({"error": "Not found"}))).into_response(),
            FsError::OutsideRoot => (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Path is outside the workspace root"})),
            )
                .into_response(),
            FsError::InvalidName => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid path or name"})),
            )
                .into_response(),
            FsError::InvalidRoot => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Workspace root is not in the allowed list"
                })),
            )
                .into_response(),
            FsError::TooLarge(size) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({
                    "error": "File too large",
                    "size": size,
                })),
            )
                .into_response(),
            FsError::Io(e) => {
                tracing::warn!(err = %e, "workspace fs error");
                let kind = e.kind();
                let status = match kind {
                    std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                    std::io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
                    std::io::ErrorKind::AlreadyExists => StatusCode::CONFLICT,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, Json(json!({"error": e.to_string()}))).into_response()
            }
        }
    }
}

impl From<std::io::Error> for FsError {
    fn from(value: std::io::Error) -> Self {
        FsError::Io(value)
    }
}

fn session_allowed_workspace_canonicals(state: &AppState) -> Vec<PathBuf> {
    let Some(ref backend) = state.session_backend else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = backend
        .list_sessions_with_metadata()
        .into_iter()
        .filter_map(|meta| {
            let wd = meta.work_dir.as_deref()?.trim();
            if wd.is_empty() {
                return None;
            }
            let p = PathBuf::from(wd);
            p.canonicalize().ok()
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn allowed_workspace_root(state: &AppState, requested: &str) -> Result<PathBuf, FsError> {
    let workspace = state.config.lock().workspace_dir.clone();
    let workspace_canonical = workspace.canonicalize().map_err(|_| FsError::InvalidRoot)?;
    let requested = PathBuf::from(requested);
    let requested_canonical = requested.canonicalize().map_err(|_| FsError::InvalidRoot)?;
    if requested_canonical == workspace_canonical {
        return Ok(workspace_canonical);
    }
    for root in session_allowed_workspace_canonicals(state) {
        if root == requested_canonical {
            return Ok(requested_canonical);
        }
    }
    Err(FsError::InvalidRoot)
}

fn resolve_within(root: &Path, rel: &str, must_exist: bool) -> Result<PathBuf, FsError> {
    let rel = rel.trim();
    if rel.is_empty() || rel == "." {
        if !must_exist {
            return Err(FsError::InvalidName);
        }
        return Ok(root.to_path_buf());
    }
    let rel_path = PathBuf::from(rel.trim_start_matches(['/', '\\']));
    if rel_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(FsError::OutsideRoot);
    }
    let joined = root.join(&rel_path);

    let resolved = if joined.exists() {
        joined.canonicalize().map_err(FsError::Io)?
    } else {
        if must_exist {
            return Err(FsError::NotFound);
        }
        let parent = joined.parent().ok_or(FsError::InvalidName)?;
        let parent_canonical = parent.canonicalize().map_err(|_| FsError::NotFound)?;
        let file_name = joined.file_name().ok_or(FsError::InvalidName)?;
        parent_canonical.join(file_name)
    };

    if !resolved.starts_with(root) {
        return Err(FsError::OutsideRoot);
    }
    Ok(resolved)
}

fn relative_path(root: &Path, target: &Path) -> String {
    target
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn modified_at(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
    Some(datetime.to_rfc3339())
}

fn entry_to_json(root: &Path, path: &Path, name: &str, is_dir: bool) -> serde_json::Value {
    let mut payload = json!({
        "name": name,
        "relPath": relative_path(root, path),
        "isDir": is_dir,
    });
    if let Ok(metadata) = std::fs::metadata(path) {
        if let Some(map) = payload.as_object_mut() {
            if !is_dir {
                map.insert("sizeBytes".into(), json!(metadata.len()));
            }
            if let Some(ts) = modified_at(&metadata) {
                map.insert("modifiedAt".into(), json!(ts));
            }
        }
    }
    payload
}

fn is_hidden_default(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    HIDDEN_DEFAULT_DIRS.contains(&name)
}

#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    pub root: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default, rename = "showHidden")]
    pub show_hidden: Option<bool>,
}

pub async fn handle_workspace_tree(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TreeQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let rel = q.path.as_deref().unwrap_or("");
    let target = match resolve_within(&root, rel, true) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    if !target.is_dir() {
        return FsError::InvalidName.into_response();
    }
    let depth = q
        .depth
        .unwrap_or(DEFAULT_TREE_DEPTH)
        .clamp(0, MAX_TREE_DEPTH);
    let show_hidden = q.show_hidden.unwrap_or(false);
    let mut budget = MAX_TREE_NODES;

    fn collect(
        root: &Path,
        dir: &Path,
        depth: u32,
        show_hidden: bool,
        budget: &mut usize,
    ) -> std::io::Result<Vec<serde_json::Value>> {
        let mut entries = Vec::new();
        let mut read = std::fs::read_dir(dir)?
            .filter_map(|res| res.ok())
            .collect::<Vec<_>>();
        read.sort_by(|a, b| {
            let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
            b_dir.cmp(&a_dir).then(
                a.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&b.file_name().to_string_lossy().to_lowercase()),
            )
        });
        for entry in read {
            if *budget == 0 {
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !show_hidden && is_hidden_default(&name) {
                continue;
            }
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let mut node = entry_to_json(root, &path, &name, is_dir);
            *budget = budget.saturating_sub(1);
            if is_dir && depth > 0 {
                if let Ok(children) = collect(root, &path, depth - 1, show_hidden, budget) {
                    if let Some(map) = node.as_object_mut() {
                        map.insert("children".into(), json!(children));
                        map.insert("loaded".into(), json!(true));
                    }
                }
            } else if is_dir {
                if let Some(map) = node.as_object_mut() {
                    map.insert("loaded".into(), json!(false));
                }
            }
            entries.push(node);
        }
        Ok(entries)
    }

    let initial_depth = depth.saturating_sub(1);
    match collect(&root, &target, initial_depth, show_hidden, &mut budget) {
        Ok(children) => Json(json!({
            "root": root.to_string_lossy(),
            "relPath": relative_path(&root, &target),
            "entries": children,
            "truncated": budget == 0,
        }))
        .into_response(),
        Err(e) => FsError::Io(e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct FileGetQuery {
    pub root: String,
    pub path: String,
}

pub async fn handle_workspace_file_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FileGetQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &q.path, true) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    if !target.is_file() {
        return FsError::InvalidName.into_response();
    }
    let target_for_io = target.clone();
    let read_result: Result<(std::fs::Metadata, Vec<u8>), FsError> =
        tokio::task::spawn_blocking(move || {
            let metadata =
                std::fs::metadata(&target_for_io).map_err(FsError::Io)?;
            if metadata.len() > MAX_FILE_READ_BYTES {
                return Err(FsError::TooLarge(metadata.len()));
            }
            let bytes = std::fs::read(&target_for_io).map_err(FsError::Io)?;
            Ok((metadata, bytes))
        })
        .await
        .unwrap_or_else(|e| {
            Err(FsError::Io(std::io::Error::other(format!(
                "blocking task join failed: {e}"
            ))))
        });
    let (metadata, bytes) = match read_result {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let is_binary = looks_binary(&bytes);
    let modified = modified_at(&metadata);
    let mime = mime_from_extension(&target);
    let payload = if is_binary {
        json!({
            "content": base64::engine::general_purpose::STANDARD.encode(&bytes),
            "encoding": "base64",
            "isBinary": true,
            "sizeBytes": metadata.len(),
            "modifiedAt": modified,
            "mimeType": mime,
        })
    } else {
        let text = match String::from_utf8(bytes.clone()) {
            Ok(s) => s,
            Err(_) => {
                return Json(json!({
                    "content": base64::engine::general_purpose::STANDARD.encode(&bytes),
                    "encoding": "base64",
                    "isBinary": true,
                    "sizeBytes": metadata.len(),
                    "modifiedAt": modified,
                    "mimeType": mime,
                }))
                .into_response();
            }
        };
        json!({
            "content": text,
            "encoding": "utf8",
            "isBinary": false,
            "sizeBytes": metadata.len(),
            "modifiedAt": modified,
            "mimeType": mime,
        })
    };
    Json(payload).into_response()
}

fn looks_binary(bytes: &[u8]) -> bool {
    let sniff_len = bytes.len().min(8_192);
    bytes[..sniff_len].iter().any(|b| *b == 0)
}

fn mime_from_extension(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "go" => "text/x-go",
        "yaml" | "yml" => "text/yaml",
        "toml" => "text/toml",
        "xml" => "text/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Deserialize)]
pub struct FilePutBody {
    pub root: String,
    pub path: String,
    pub content: String,
    #[serde(default, rename = "ifMatchMtime")]
    pub if_match_mtime: Option<String>,
    #[serde(default)]
    pub encoding: Option<String>,
}

pub async fn handle_workspace_file_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FilePutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    write_file_inner(&state, body,  false).await
}

pub async fn handle_workspace_file_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FilePutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    write_file_inner(&state, body,  true).await
}

async fn write_file_inner(
    state: &AppState,
    body: FilePutBody,
    create_only: bool,
) -> axum::response::Response {
    let root = match allowed_workspace_root(state, &body.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &body.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    if create_only && target.exists() {
        return FsError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "File already exists",
        ))
        .into_response();
    }

    let bytes = match body.encoding.as_deref() {
        Some("base64") => match base64::engine::general_purpose::STANDARD.decode(body.content.as_bytes()) {
            Ok(b) => b,
            Err(_) => return FsError::InvalidName.into_response(),
        },
        _ => body.content.as_bytes().to_vec(),
    };

    let target_for_io = target.clone();
    let if_match = body.if_match_mtime.clone();
    let bytes_for_io = bytes;
    enum WriteOutcome {
        Conflict(String),
        Ok(Option<std::fs::Metadata>),
        Err(std::io::Error),
    }
    let outcome = tokio::task::spawn_blocking(move || -> WriteOutcome {
        if !create_only {
            if let Some(expected) = if_match.as_deref() {
                if target_for_io.exists() {
                    if let Ok(meta) = std::fs::metadata(&target_for_io) {
                        if let Some(actual) = modified_at(&meta) {
                            if actual != expected {
                                return WriteOutcome::Conflict(actual);
                            }
                        }
                    }
                }
            }
        }

        if let Some(parent) = target_for_io.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return WriteOutcome::Err(e);
                }
            }
        }

        if let Err(e) = atomic_write(&target_for_io, &bytes_for_io) {
            return WriteOutcome::Err(e);
        }

        WriteOutcome::Ok(std::fs::metadata(&target_for_io).ok())
    })
    .await
    .unwrap_or_else(|e| {
        WriteOutcome::Err(std::io::Error::other(format!(
            "blocking task join failed: {e}"
        )))
    });

    match outcome {
        WriteOutcome::Conflict(actual) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "File was modified by another process",
                "actualMtime": actual,
            })),
        )
            .into_response(),
        WriteOutcome::Err(e) => FsError::Io(e).into_response(),
        WriteOutcome::Ok(metadata) => {
            let payload = json!({
                "ok": true,
                "relPath": relative_path(&root, &target),
                "sizeBytes": metadata.as_ref().map(|m| m.len()),
                "modifiedAt": metadata.as_ref().and_then(modified_at),
            });
            (StatusCode::OK, Json(payload)).into_response()
        }
    }
}

fn atomic_write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent")
    })?;
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tmp");
    let tmp = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all().ok();
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct DirPostBody {
    pub root: String,
    pub path: String,
}

pub async fn handle_workspace_dir_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DirPostBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &body.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    if let Err(e) = std::fs::create_dir_all(&target) {
        return FsError::Io(e).into_response();
    }
    Json(json!({
        "ok": true,
        "relPath": relative_path(&root, &target),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct MoveBody {
    pub root: String,
    #[serde(rename = "fromPath")]
    pub from_path: String,
    #[serde(rename = "toPath")]
    pub to_path: String,
}

pub async fn handle_workspace_move(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MoveBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let from = match resolve_within(&root, &body.from_path, true) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let to = match resolve_within(&root, &body.to_path, false) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if to.exists() {
        return FsError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Destination already exists",
        ))
        .into_response();
    }
    if let Some(parent) = to.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return FsError::Io(e).into_response();
            }
        }
    }
    if let Err(e) = std::fs::rename(&from, &to) {
        return FsError::Io(e).into_response();
    }
    Json(json!({
        "ok": true,
        "fromPath": relative_path(&root, &from),
        "toPath": relative_path(&root, &to),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    pub root: String,
    pub path: String,
    #[serde(default)]
    pub recursive: Option<bool>,
}

pub async fn handle_workspace_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DeleteQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &q.path, true) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if target == root {
        return FsError::OutsideRoot.into_response();
    }
    let metadata = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(e) => return FsError::Io(e).into_response(),
    };
    let result = if metadata.is_dir() {
        if q.recursive.unwrap_or(false) {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_dir(&target)
        }
    } else {
        std::fs::remove_file(&target)
    };
    if let Err(e) = result {
        return FsError::Io(e).into_response();
    }
    Json(json!({"ok": true})).into_response()
}

#[derive(Debug, Deserialize)]
pub struct UploadBody {
    pub root: String,
    pub path: String,
    #[serde(rename = "contentBase64")]
    pub content_base64: String,
    #[serde(default)]
    pub overwrite: Option<bool>,
}

pub async fn handle_workspace_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UploadBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &body.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    if target.exists() && !body.overwrite.unwrap_or(false) {
        return FsError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "File already exists",
        ))
        .into_response();
    }
    let bytes = match base64::engine::general_purpose::STANDARD.decode(body.content_base64.as_bytes()) {
        Ok(b) => b,
        Err(_) => return FsError::InvalidName.into_response(),
    };
    if bytes.len() > MAX_UPLOAD_BYTES {
        return FsError::TooLarge(bytes.len() as u64).into_response();
    }
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return FsError::Io(e).into_response();
            }
        }
    }
    if let Err(e) = atomic_write(&target, &bytes) {
        return FsError::Io(e).into_response();
    }
    let metadata = std::fs::metadata(&target).ok();
    Json(json!({
        "ok": true,
        "relPath": relative_path(&root, &target),
        "sizeBytes": metadata.as_ref().map(|m| m.len()),
        "modifiedAt": metadata.as_ref().and_then(modified_at),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub root: String,
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "showHidden")]
    pub show_hidden: Option<bool>,
}

pub async fn handle_workspace_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let needle = q.query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return Json(json!({"results": [], "total": 0})).into_response();
    }
    let limit = q.limit.unwrap_or(MAX_SEARCH_RESULTS).min(MAX_SEARCH_RESULTS);
    let show_hidden = q.show_hidden.unwrap_or(false);

    let mut results: Vec<serde_json::Value> = Vec::new();
    walk_filenames(&root, &root, &needle, show_hidden, limit, &mut results);
    let total = results.len();
    Json(json!({
        "results": results,
        "total": total,
        "limit": limit,
    }))
    .into_response()
}

fn walk_filenames(
    root: &Path,
    dir: &Path,
    needle: &str,
    show_hidden: bool,
    cap: usize,
    out: &mut Vec<serde_json::Value>,
) {
    if out.len() >= cap {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        if out.len() >= cap {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && is_hidden_default(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if name.to_ascii_lowercase().contains(needle) {
            out.push(entry_to_json(root, &path, &name, is_dir));
        }
        if is_dir {
            walk_filenames(root, &path, needle, show_hidden, cap, out);
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WatchQuery {
    pub root: String,
}

#[cfg(feature = "fs-watch")]
pub async fn handle_workspace_watch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WatchQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let cache = state.git_status_cache.clone();
    match watch_impl::start_watch_stream(root, cache).await {
        Ok(sse) => sse.into_response(),
        Err(e) => {
            tracing::warn!(err = %e, "workspace watch start failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

#[cfg(not(feature = "fs-watch"))]
pub async fn handle_workspace_watch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(_q): Query<WatchQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "fs-watch feature is disabled in this build",
        })),
    )
        .into_response()
}

#[cfg(feature = "fs-watch")]
mod watch_impl {
    use super::relative_path;
    use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
    use futures_util::future::OptionFuture;
    use notify::event::{EventKind, ModifyKind, RenameMode};
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
    use serde_json::json;
    use std::collections::{HashMap, VecDeque};
    use std::convert::Infallible;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::Stream;

    const DEBOUNCE_MS: u64 = 100;

    const RENAME_WINDOW_MS: u64 = 800;

    const RECENT_REMOVED_TTL_MS: u64 = 1500;

    const RECENT_REMOVED_CAP: usize = 32;

    const CHANNEL_SIZE: usize = 64;

    const KEEP_ALIVE_SECS: u64 = 15;

    #[derive(Clone)]
    struct PendingEvent {
        kind: &'static str,
        from_rel: Option<String>,
    }

    struct RecentRemoved {
        rel: String,
        basename: String,
        parent: String,
        expires_at: Instant,
    }

    fn split_rel(rel: &str) -> (String, String) {
        if let Some(idx) = rel.rfind('/') {
            (rel[..idx].to_string(), rel[idx + 1..].to_string())
        } else {
            (String::new(), rel.to_string())
        }
    }

    fn push_recent_removed(buf: &mut VecDeque<RecentRemoved>, rel: &str) {
        let (parent, basename) = split_rel(rel);
        if basename.is_empty() {
            return;
        }
        while buf.len() >= RECENT_REMOVED_CAP {
            buf.pop_front();
        }
        buf.push_back(RecentRemoved {
            rel: rel.to_string(),
            basename,
            parent,
            expires_at: Instant::now() + Duration::from_millis(RECENT_REMOVED_TTL_MS),
        });
    }

    fn match_recent_removed(
        buf: &mut VecDeque<RecentRemoved>,
        to_rel: &str,
    ) -> Option<String> {
        let (to_parent, to_base) = split_rel(to_rel);
        if to_base.is_empty() {
            return None;
        }
        if let Some(idx) = buf
            .iter()
            .position(|r| r.basename == to_base && r.parent == to_parent)
        {
            return buf.remove(idx).map(|r| r.rel);
        }
        if let Some(idx) = buf.iter().position(|r| r.basename == to_base) {
            return buf.remove(idx).map(|r| r.rel);
        }
        None
    }

    fn compute_next_deadline(
        pending_renames: &HashMap<usize, (String, Instant)>,
        recent_removed: &VecDeque<RecentRemoved>,
    ) -> Option<tokio::time::Instant> {
        let mut next: Option<Instant> = None;
        for (_, ts) in pending_renames.values() {
            let exp = *ts + Duration::from_millis(RENAME_WINDOW_MS);
            next = Some(next.map_or(exp, |n: Instant| n.min(exp)));
        }
        for r in recent_removed {
            next = Some(next.map_or(r.expires_at, |n: Instant| n.min(r.expires_at)));
        }
        next.map(|i| {
            let remaining = i.saturating_duration_since(Instant::now());
            tokio::time::Instant::now() + remaining
        })
    }

    pub(super) async fn start_watch_stream(
        root: PathBuf,
        git_status_cache: crate::gateway::git_routes::GitStatusCache,
    ) -> std::io::Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
        let (raw_tx, mut raw_rx) =
            tokio::sync::mpsc::unbounded_channel::<notify::Result<notify::Event>>();
        let config = Config::default().with_poll_interval(Duration::from_millis(500));
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {

                let _ = raw_tx.send(res);
            },
            config,
        )
        .map_err(|e| std::io::Error::other(format!("notify init: {e}")))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| std::io::Error::other(format!("notify watch: {e}")))?;

        let (out_tx, out_rx) =
            tokio::sync::mpsc::channel::<Result<SseEvent, Infallible>>(CHANNEL_SIZE);

        tokio::spawn(async move {

            let _watcher_guard = watcher;
            let mut pending: HashMap<String, PendingEvent> = HashMap::new();
            let mut pending_renames: HashMap<usize, (String, Instant)> = HashMap::new();
            let mut recent_removed: VecDeque<RecentRemoved> =
                VecDeque::with_capacity(RECENT_REMOVED_CAP);
            let mut deadline: Option<tokio::time::Instant> = None;

            loop {
                let sleep_fut: OptionFuture<_> =
                    deadline.map(tokio::time::sleep_until).into();
                tokio::pin!(sleep_fut);

                tokio::select! {
                    biased;
                    Some(_) = &mut sleep_fut => {
                        let now = Instant::now();
                        let stale_keys: Vec<usize> = pending_renames
                            .iter()
                            .filter_map(|(k, (_, ts))| {
                                if now.duration_since(*ts)
                                    >= Duration::from_millis(RENAME_WINDOW_MS)
                                {
                                    Some(*k)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        for key in stale_keys {
                            if let Some((rel, _)) = pending_renames.remove(&key) {
                                push_recent_removed(&mut recent_removed, &rel);
                                pending.entry(rel).or_insert(PendingEvent {
                                    kind: "removed",
                                    from_rel: None,
                                });
                            }
                        }
                        while let Some(front) = recent_removed.front() {
                            if front.expires_at <= now {
                                recent_removed.pop_front();
                            } else {
                                break;
                            }
                        }
                        let drained: Vec<(String, PendingEvent)> =
                            pending.drain().collect();
                        deadline =
                            compute_next_deadline(&pending_renames, &recent_removed);
                        for (rel, info) in drained {
                            let mut payload = json!({
                                "kind": info.kind,
                                "relPath": rel,
                            });
                            if let Some(from) = info.from_rel {
                                payload["fromRelPath"] = json!(from);
                            }
                            let event = SseEvent::default().data(payload.to_string());
                            if out_tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                    }
                    maybe_event = raw_rx.recv() => {
                        let Some(res) = maybe_event else { return };
                        let event = match res {
                            Ok(ev) => ev,
                            Err(err) => {
                                tracing::warn!(target: "sen::workspace_watch", error = %err, "notify backend error");
                                continue;
                            }
                        };
                        let mut should_invalidate_git = false;
                        let rels: Vec<String> = event
                            .paths
                            .iter()
                            .map(|p| relative_path(&root, p))
                            .filter(|r| !r.is_empty())
                            .collect();
                        for rel in &rels {
                            if !rel.starts_with(".git/") && rel != ".git" {
                                should_invalidate_git = true;
                            }
                        }
                        let mut handled = false;
                        match &event.kind {
                            EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                                if rels.len() == 2 =>
                            {
                                let from = rels[0].clone();
                                let to = rels[1].clone();
                                pending.insert(
                                    to,
                                    PendingEvent {
                                        kind: "renamed",
                                        from_rel: Some(from),
                                    },
                                );
                                handled = true;
                            }
                            EventKind::Modify(ModifyKind::Name(RenameMode::From))
                                if !rels.is_empty() =>
                            {
                                let from = rels[0].clone();
                                if let Some(tracker) = event.attrs.tracker() {
                                    pending_renames
                                        .insert(tracker, (from, Instant::now()));
                                    handled = true;
                                } else {
                                    push_recent_removed(&mut recent_removed, &from);
                                    pending.insert(
                                        from,
                                        PendingEvent {
                                            kind: "removed",
                                            from_rel: None,
                                        },
                                    );
                                    handled = true;
                                }
                            }
                            EventKind::Modify(ModifyKind::Name(RenameMode::To))
                                if !rels.is_empty() =>
                            {
                                let to = rels[0].clone();
                                let paired = event
                                    .attrs
                                    .tracker()
                                    .and_then(|t| pending_renames.remove(&t));
                                if let Some((from, _)) = paired {
                                    pending.insert(
                                        to,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: Some(from),
                                        },
                                    );
                                } else if let Some(from) =
                                    match_recent_removed(&mut recent_removed, &to)
                                {
                                    pending.insert(
                                        to,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: Some(from),
                                        },
                                    );
                                } else {
                                    pending.insert(
                                        to,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: None,
                                        },
                                    );
                                }
                                handled = true;
                            }
                            _ => {}
                        }
                        if !handled {
                            let Some(kind_str) = classify_event_kind(&event.kind) else {
                                if should_invalidate_git {
                                    crate::gateway::git_routes::invalidate_root(
                                        &git_status_cache,
                                        &root,
                                    );
                                }
                                continue;
                            };
                            for rel in rels {
                                let matched_from = if kind_str == "created" {
                                    match_recent_removed(&mut recent_removed, &rel)
                                } else {
                                    None
                                };
                                if let Some(from) = matched_from {
                                    pending.insert(
                                        rel,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: Some(from),
                                        },
                                    );
                                } else {
                                    pending
                                        .entry(rel)
                                        .and_modify(|existing| {
                                            if existing.kind != "renamed" {
                                                existing.kind = kind_str;
                                            }
                                        })
                                        .or_insert(PendingEvent {
                                            kind: kind_str,
                                            from_rel: None,
                                        });
                                }
                            }
                        }
                        if should_invalidate_git {
                            crate::gateway::git_routes::invalidate_root(
                                &git_status_cache,
                                &root,
                            );
                        }
                        let needs_deadline = !pending.is_empty()
                            || !pending_renames.is_empty()
                            || !recent_removed.is_empty();
                        if deadline.is_none() && needs_deadline {
                            let wait = if !pending.is_empty() {
                                DEBOUNCE_MS
                            } else if !pending_renames.is_empty() {
                                RENAME_WINDOW_MS
                            } else {
                                RECENT_REMOVED_TTL_MS
                            };
                            deadline = Some(
                                tokio::time::Instant::now()
                                    + Duration::from_millis(wait),
                            );
                        }
                    }
                }
            }
        });

        let stream = ReceiverStream::new(out_rx);
        Ok(Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(KEEP_ALIVE_SECS))
                .text("keep-alive"),
        ))
    }

    fn classify_event_kind(kind: &EventKind) -> Option<&'static str> {
        match kind {
            EventKind::Create(_) => Some("created"),
            EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Metadata(_)) => Some("modified"),
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => Some("removed"),
            EventKind::Modify(ModifyKind::Name(_)) => Some("renamed"),
            EventKind::Modify(_) => Some("modified"),
            EventKind::Remove(_) => Some("removed"),

            EventKind::Access(notify::event::AccessKind::Close(_)) => Some("modified"),
            _ => None,
        }
    }
}
