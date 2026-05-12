// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use super::api::require_auth;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Json, Response},
};
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub const GIT_STATUS_CACHE_TTL_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct GitStatusEntry {
    pub rel_path: String,
    pub index: char,
    pub worktree: char,
    pub orig_rel_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CachedGitStatus {
    pub is_repo: bool,
    pub entries: Vec<GitStatusEntry>,
    pub computed_at_ms: i64,
    pub last_invalidated_at_ms: i64,
    pub etag: String,
}

pub type GitStatusCache = Arc<RwLock<HashMap<PathBuf, CachedGitStatus>>>;

pub fn new_git_status_cache() -> GitStatusCache {
    Arc::new(RwLock::new(HashMap::new()))
}

pub fn invalidate_root(cache: &GitStatusCache, root: &Path) {
    let now_ms = current_millis();
    let mut guard = cache.write();
    if let Some(entry) = guard.get_mut(root) {
        entry.last_invalidated_at_ms = now_ms;
    }
}

fn current_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn normalize_etag(raw: &str) -> Cow<'_, str> {
    let trimmed = raw.trim();
    let without_weak: &str = match trimmed.get(..2) {
        Some(prefix) if prefix.eq_ignore_ascii_case("w/") => &trimmed[2..],
        _ => trimmed,
    };
    Cow::Borrowed(without_weak.trim().trim_matches('"'))
}

#[derive(Debug, Deserialize)]
pub struct GitStatusQuery {
    pub root: String,
    #[serde(default, rename = "forceRefresh")]
    pub force_refresh: Option<bool>,
}

pub async fn handle_git_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<GitStatusQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match resolve_root(&state, &q.root) {
        Some(p) => p,
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Workspace root is not in the allowed list"
                })),
            )
                .into_response();
        }
    };
    let force = q.force_refresh.unwrap_or(false);
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| normalize_etag(s).into_owned());
    let now_ms = current_millis();
    if !force {
        if let Some(cached) = state.git_status_cache.read().get(&root).cloned() {
            let age_ms = now_ms.saturating_sub(cached.computed_at_ms);
            let invalidated = cached.last_invalidated_at_ms > cached.computed_at_ms;
            if !invalidated && age_ms < (GIT_STATUS_CACHE_TTL_SECS as i64) * 1000 {
                return build_status_response(&cached, if_none_match.as_deref());
            }
        }
    }
    let computed = match compute_git_status(&root).await {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(error = %err, root = %root.display(), "git status compute error");
            let etag = compute_entries_etag(false, &[]);
            let fallback = CachedGitStatus {
                is_repo: false,
                entries: Vec::new(),
                computed_at_ms: now_ms,
                last_invalidated_at_ms: 0,
                etag,
            };
            return build_status_response(&fallback, if_none_match.as_deref());
        }
    };
    state
        .git_status_cache
        .write()
        .insert(root.clone(), computed.clone());
    build_status_response(&computed, if_none_match.as_deref())
}

fn build_status_response(cached: &CachedGitStatus, if_none_match: Option<&str>) -> Response {
    let normalized_match = if_none_match.map(normalize_etag);
    let etag_header = format!("\"{}\"", cached.etag);
    let header_value = HeaderValue::from_str(&etag_header)
        .unwrap_or_else(|_| HeaderValue::from_static("\"0\""));
    if normalized_match.as_deref() == Some(cached.etag.as_str()) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        response.headers_mut().insert(header::ETAG, header_value);
        return response;
    }
    let mut response = Json(serialize_status(cached)).into_response();
    response.headers_mut().insert(header::ETAG, header_value);
    response
}

fn compute_entries_etag(is_repo: bool, entries: &[GitStatusEntry]) -> String {
    let mut sorted: Vec<&GitStatusEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    let mut hasher = Sha256::new();
    hasher.update(if is_repo { b"1\n" } else { b"0\n" });
    for entry in sorted {
        hasher.update(entry.rel_path.as_bytes());
        hasher.update(b"\0");
        let chars = [entry.index as u8, entry.worktree as u8];
        hasher.update(chars);
        hasher.update(b"\0");
        if let Some(orig) = entry.orig_rel_path.as_deref() {
            hasher.update(orig.as_bytes());
        }
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    for byte in &digest[..8] {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn resolve_root(state: &AppState, requested: &str) -> Option<PathBuf> {
    let requested_path = PathBuf::from(requested);
    let canonical = requested_path.canonicalize().ok()?;
    let workspace = state.config.lock().workspace_dir.clone();
    if let Ok(canon_ws) = workspace.canonicalize() {
        if canon_ws == canonical {
            return Some(canonical);
        }
    }
    if let Some(backend) = state.session_backend.as_ref() {
        for meta in backend.list_sessions_with_metadata() {
            let Some(wd) = meta.work_dir.as_deref() else {
                continue;
            };
            let trimmed = wd.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(p) = PathBuf::from(trimmed).canonicalize() {
                if p == canonical {
                    return Some(canonical);
                }
            }
        }
    }
    None
}

fn serialize_status(cached: &CachedGitStatus) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = cached
        .entries
        .iter()
        .map(|entry| {
            let mut payload = json!({
                "relPath": entry.rel_path,
                "index": entry.index.to_string(),
                "worktree": entry.worktree.to_string(),
            });
            if let Some(orig) = entry.orig_rel_path.as_deref() {
                payload["origRelPath"] = json!(orig);
            }
            payload
        })
        .collect();
    json!({
        "isRepo": cached.is_repo,
        "entries": entries,
        "computedAt": cached.computed_at_ms,
        "etag": cached.etag,
    })
}

async fn compute_git_status(root: &Path) -> std::io::Result<CachedGitStatus> {
    let now_ms = current_millis();
    let output = crate::util::hidden_async_command("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "-z",
            "--ignored=no",
            "--untracked-files=normal",
        ])
        .output()
        .await?;

    if !output.status.success() {
        let etag = compute_entries_etag(false, &[]);
        return Ok(CachedGitStatus {
            is_repo: false,
            entries: Vec::new(),
            computed_at_ms: now_ms,
            last_invalidated_at_ms: 0,
            etag,
        });
    }

    let entries = parse_porcelain_z(&output.stdout);
    let etag = compute_entries_etag(true, &entries);
    Ok(CachedGitStatus {
        is_repo: true,
        entries,
        computed_at_ms: now_ms,
        last_invalidated_at_ms: 0,
        etag,
    })
}

fn parse_porcelain_z(bytes: &[u8]) -> Vec<GitStatusEntry> {
    let mut entries: Vec<GitStatusEntry> = Vec::new();
    let mut idx = 0usize;
    let len = bytes.len();
    while idx < len {
        let end = match bytes[idx..].iter().position(|b| *b == 0) {
            Some(pos) => idx + pos,
            None => break,
        };
        let token = &bytes[idx..end];
        idx = end + 1;
        if token.len() < 3 {
            continue;
        }
        let index_byte = token[0];
        let worktree_byte = token[1];
        if token[2] != b' ' {
            continue;
        }
        let path_bytes = &token[3..];
        let path = match std::str::from_utf8(path_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => String::from_utf8_lossy(path_bytes).into_owned(),
        };
        let is_rename = index_byte == b'R' || worktree_byte == b'R'
            || index_byte == b'C' || worktree_byte == b'C';
        let mut entry = GitStatusEntry {
            rel_path: normalize_rel(&path),
            index: normalize_status_char(index_byte as char),
            worktree: normalize_status_char(worktree_byte as char),
            orig_rel_path: None,
        };
        if is_rename {
            if idx < len {
                let end2 = match bytes[idx..].iter().position(|b| *b == 0) {
                    Some(pos) => idx + pos,
                    None => len,
                };
                let orig = &bytes[idx..end2];
                idx = end2 + 1;
                let orig_str = match std::str::from_utf8(orig) {
                    Ok(s) => s.to_string(),
                    Err(_) => String::from_utf8_lossy(orig).into_owned(),
                };
                entry.orig_rel_path = Some(normalize_rel(&orig_str));
            }
        }
        entries.push(entry);
    }
    entries
}

fn normalize_rel(p: &str) -> String {
    p.replace('\\', "/")
}

fn normalize_status_char(c: char) -> char {
    match c {
        ' ' => ' ',
        'M' | 'A' | 'D' | 'R' | 'C' | 'U' | 'T' | '?' | '!' => c,
        _ => c,
    }
}
