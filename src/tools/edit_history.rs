// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

const MAX_SNAPSHOTS_PER_FILE: usize = 100;
const MAX_TIMELINE_EVENTS: usize = 4096;
const HISTORY_DIR_NAME: &str = ".sen/edit_history";
const INDEX_FILE: &str = "index.json";

const SESSION_KEY_SEP: char = '\u{1f}';

fn scoped_session_key(session: &str, rel: &str) -> String {
    format!("{session}{SESSION_KEY_SEP}{rel}")
}

fn rel_from_scoped(key: &str) -> Option<&str> {
    key.split_once(SESSION_KEY_SEP).map(|(_, rel)| rel)
}

fn scoped_matches_bucket(key: &str, bucket: &str) -> bool {
    key.split_once(SESSION_KEY_SEP)
        .map(|(session, _)| session == bucket)
        .unwrap_or(false)
}

static SHARED_HISTORIES: once_cell::sync::Lazy<
    parking_lot::Mutex<HashMap<PathBuf, Arc<EditHistory>>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(HashMap::new()));

fn acquire_write_guard_blocking(path: &Path) -> Option<crate::session::ResourceGuard> {
    let handle = tokio::runtime::Handle::try_current().ok()?;
    if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::CurrentThread {
        return None;
    }
    let result = tokio::task::block_in_place(|| {
        handle.block_on(crate::session::acquire_file_write_guard(path))
    });
    match result {
        Ok(guard) => guard,
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "edit_history: failed to acquire file write lock; proceeding without it"
            );
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub sha256: String,
    pub timestamp: u64,
    pub tool_name: String,
    pub description: String,
    pub byte_size: usize,

    #[serde(default)]
    pub absent: bool,

    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditEvent {
    pub path: String,
    pub snapshot_index: usize,
    pub timestamp: u64,
    pub tool_name: String,
    pub description: String,

    #[serde(default)]
    pub edit_batch_id: Option<String>,

    #[serde(default)]
    pub post_sha256: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RevertOutcome {
    pub reverted: Vec<String>,
    pub skipped_stale: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionEditedFile {
    pub rel_path: String,
    pub first_snapshot_index: usize,
    pub pre_image: FileSnapshot,
    pub batch_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryIndex {
    files: HashMap<String, Vec<FileSnapshot>>,

    #[serde(default)]
    timeline: Vec<EditEvent>,
}

pub struct EditHistory {
    storage_dir: PathBuf,
    workspace_dir: PathBuf,
    state: RwLock<EditHistoryState>,
}

#[derive(Debug, Default)]
#[allow(dead_code)]
struct EditHistoryState {
    index: HistoryIndex,
    timeline: Vec<EditEvent>,
    session_id: String,
    loaded: bool,

    session_first_index: HashMap<String, usize>,
}

impl EditHistory {
    pub fn shared_for_workspace(workspace_dir: &Path) -> Arc<Self> {
        let key = crate::util::normalize_path_for_containment(workspace_dir);
        let mut map = SHARED_HISTORIES.lock();
        if let Some(history) = map.get(&key) {
            return Arc::clone(history);
        }
        let history = Self::new(key.clone());
        map.insert(key, Arc::clone(&history));
        history
    }

    pub fn new(workspace_dir: PathBuf) -> Arc<Self> {
        let storage_dir = workspace_dir.join(HISTORY_DIR_NAME);
        let session_id = format!(
            "session-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        Arc::new(Self {
            storage_dir,
            workspace_dir,
            state: RwLock::new(EditHistoryState {
                session_id,
                ..Default::default()
            }),
        })
    }

    fn ensure_loaded(&self) {
        let mut state = self.state.write();
        if state.loaded {
            return;
        }
        let _ = std::fs::create_dir_all(&self.storage_dir);
        let index_path = self.storage_dir.join(INDEX_FILE);
        if let Ok(data) = std::fs::read_to_string(&index_path) {
            if let Ok(mut idx) = serde_json::from_str::<HistoryIndex>(&data) {
                state.timeline = std::mem::take(&mut idx.timeline);
                state.index = idx;
            }
        }
        state.loaded = true;
    }

    fn save_index(&self) {
        let index_path = self.storage_dir.join(INDEX_FILE);
        let serialized = {
            let state = self.state.read();
            let start = state.timeline.len().saturating_sub(MAX_TIMELINE_EVENTS);
            let snapshot = HistoryIndex {
                files: state.index.files.clone(),
                timeline: state.timeline[start..].to_vec(),
            };
            serde_json::to_string_pretty(&snapshot)
        };
        if let Ok(json) = serialized {
            if let Err(e) = crate::util::atomic_write(&index_path, json.as_bytes()) {
                tracing::warn!(
                    path = %index_path.display(),
                    error = %e,
                    "failed to persist edit-history index"
                );
            }
        }
    }

    fn sha256(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("{:x}", hasher.finalize())
    }

    fn relative_key(&self, path: &Path) -> String {
        crate::util::path_relative_to(path, &self.workspace_dir)
            .unwrap_or_else(|| path.to_path_buf())
            .to_string_lossy()
            .replace('\\', "/")
    }

    pub fn snapshot_before_write(
        &self,
        path: &Path,
        tool_name: &str,
        description: &str,
    ) -> anyhow::Result<()> {
        self.snapshot_before_write_with_batch(path, tool_name, description, None)
    }

    pub fn snapshot_before_write_with_batch(
        &self,
        path: &Path,
        tool_name: &str,
        description: &str,
        edit_batch_id: Option<String>,
    ) -> anyhow::Result<()> {
        self.ensure_loaded();

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let ctx_session = crate::session::current_session_context()
            .map(|c| c.session_id)
            .filter(|s| !s.is_empty());

        let (snapshot, is_creation) = if path.exists() {
            let content = std::fs::read(path)?;
            let hash = Self::sha256(&content);
            let content_path = self.storage_dir.join(&hash);
            if !content_path.exists() {
                std::fs::write(&content_path, &content)?;
            }
            (
                FileSnapshot {
                    sha256: hash,
                    timestamp: now,
                    tool_name: tool_name.to_string(),
                    description: description.to_string(),
                    byte_size: content.len(),
                    absent: false,
                    session_id: ctx_session.clone(),
                },
                false,
            )
        } else {
            (
                FileSnapshot {
                    sha256: String::new(),
                    timestamp: now,
                    tool_name: tool_name.to_string(),
                    description: description.to_string(),
                    byte_size: 0,
                    absent: true,
                    session_id: ctx_session.clone(),
                },
                true,
            )
        };
        let _ = is_creation;

        let key = self.relative_key(path);

        let event = EditEvent {
            path: key.clone(),
            snapshot_index: 0,
            timestamp: now,
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            edit_batch_id: edit_batch_id.clone(),
            post_sha256: None,
        };

        {
            let mut state = self.state.write();
            let chain = state.index.files.entry(key.clone()).or_default();

            if let Some(last) = chain.last() {
                if last.absent == snapshot.absent && last.sha256 == snapshot.sha256 {
                    return Ok(());
                }
            }

            chain.push(snapshot);

            let mut evicted = 0usize;
            while chain.len() > MAX_SNAPSHOTS_PER_FILE {
                chain.remove(0);
                evicted += 1;
            }

            let idx = chain.len().saturating_sub(1);

            if evicted > 0 {
                state.timeline.retain_mut(|ev| {
                    if ev.path != key {
                        return true;
                    }
                    if ev.snapshot_index < evicted {
                        return false;
                    }
                    ev.snapshot_index -= evicted;
                    true
                });
                let suffix = format!("{SESSION_KEY_SEP}{key}");
                state.session_first_index.retain(|k, first| {
                    if !k.ends_with(&suffix) {
                        return true;
                    }
                    if *first >= evicted {
                        *first -= evicted;
                        true
                    } else {
                        tracing::debug!(
                            target: "rewind",
                            path = %key,
                            "session-start pre-image evicted by snapshot cap; \
                             revert_all_session will skip this path"
                        );
                        false
                    }
                });
            }

            let bucket = ctx_session
                .clone()
                .unwrap_or_else(|| state.session_id.clone());
            state
                .session_first_index
                .entry(scoped_session_key(&bucket, &key))
                .or_insert(idx);

            let mut ev = event;
            ev.snapshot_index = idx;
            state.timeline.push(ev);

            let overflow = state.timeline.len().saturating_sub(MAX_TIMELINE_EVENTS * 2);
            if overflow > 0 {
                state.timeline.drain(0..overflow);
            }
        }

        self.save_index();
        Ok(())
    }

    pub async fn snapshot_before_write_async(
        self: &Arc<Self>,
        path: PathBuf,
        tool_name: String,
        description: String,
    ) -> anyhow::Result<()> {
        let this = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            this.snapshot_before_write(&path, &tool_name, &description)
        })
        .await?
    }

    pub fn revert_file(&self, path: &Path, snapshot_index: usize) -> anyhow::Result<()> {
        self.ensure_loaded();
        let key = self.relative_key(path);

        let (hash, absent) = {
            let state = self.state.read();
            let chain = state
                .index
                .files
                .get(&key)
                .ok_or_else(|| anyhow::anyhow!("No edit history for: {key}"))?;
            let snap = chain
                .get(snapshot_index)
                .ok_or_else(|| anyhow::anyhow!("Snapshot index {snapshot_index} out of range"))?;
            (snap.sha256.clone(), snap.absent)
        };

        self.apply_pre_image(path, &hash, absent)
    }

    fn session_bucket(&self, explicit: Option<&str>) -> String {
        if let Some(s) = explicit {
            if !s.is_empty() {
                return s.to_string();
            }
        }
        if let Some(ctx) = crate::session::current_session_context() {
            if !ctx.session_id.is_empty() {
                return ctx.session_id;
            }
        }
        self.state.read().session_id.clone()
    }

    pub fn revert_to_session_start(
        &self,
        path: &Path,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.ensure_loaded();
        let key = self.relative_key(path);
        let bucket = self.session_bucket(session_id);
        let scoped = scoped_session_key(&bucket, &key);

        let (hash, absent) = {
            let state = self.state.read();
            let idx = *state
                .session_first_index
                .get(&scoped)
                .ok_or_else(|| anyhow::anyhow!("No session edit history for: {key}"))?;
            let snap = state
                .index
                .files
                .get(&key)
                .and_then(|chain| chain.get(idx))
                .ok_or_else(|| {
                    anyhow::anyhow!("Session pre-image snapshot missing for: {key}")
                })?;
            (snap.sha256.clone(), snap.absent)
        };

        self.apply_pre_image(path, &hash, absent)
    }

    fn apply_pre_image(&self, path: &Path, hash: &str, absent: bool) -> anyhow::Result<()> {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_dir.join(path)
        };

        let _write_guard = acquire_write_guard_blocking(&abs_path);

        if absent {
            match std::fs::remove_file(&abs_path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "failed to delete created file during revert ({}): {e}",
                        abs_path.display()
                    ));
                }
            }
            return Ok(());
        }

        let content_path = self.storage_dir.join(hash);
        let content = std::fs::read(&content_path)
            .map_err(|e| anyhow::anyhow!("Snapshot content missing ({hash}): {e}"))?;

        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::util::atomic_write(&abs_path, &content)?;
        crate::session::record_write_for_current_session(&abs_path);

        Ok(())
    }

    pub fn revert_to_latest(&self, path: &Path) -> anyhow::Result<()> {
        self.ensure_loaded();
        let key = self.relative_key(path);

        let idx = {
            let state = self.state.read();
            let chain = state
                .index
                .files
                .get(&key)
                .ok_or_else(|| anyhow::anyhow!("No edit history for: {key}"))?;
            if chain.is_empty() {
                anyhow::bail!("No snapshots available for: {key}");
            }
            chain.len() - 1
        };

        self.revert_file(path, idx)
    }

    pub fn revert_all_session(&self) -> anyhow::Result<Vec<String>> {
        self.ensure_loaded();
        let bucket = self.session_bucket(None);
        let mut reverted = Vec::new();

        let targets: Vec<String> = {
            let state = self.state.read();
            state
                .session_first_index
                .keys()
                .filter(|k| scoped_matches_bucket(k, &bucket))
                .filter_map(|k| rel_from_scoped(k).map(String::from))
                .collect()
        };

        for key in targets {
            let abs_path = self.workspace_dir.join(&key);
            match self.revert_to_session_start(&abs_path, Some(&bucket)) {
                Ok(()) => reverted.push(key),
                Err(e) => {
                    tracing::warn!(
                        target: "rewind",
                        path = %abs_path.display(),
                        error = %e,
                        "revert_all_session: failed to revert file"
                    );
                }
            }
        }

        Ok(reverted)
    }

    pub fn get_file_history(&self, path: &Path) -> Vec<FileSnapshot> {
        self.ensure_loaded();
        let key = self.relative_key(path);
        let state = self.state.read();
        state.index.files.get(&key).cloned().unwrap_or_default()
    }

    pub fn files_summary(&self) -> Vec<(String, usize, u64)> {
        self.ensure_loaded();
        let state = self.state.read();
        let mut out: Vec<(String, usize, u64)> = state
            .index
            .files
            .iter()
            .filter(|(_, chain)| !chain.is_empty())
            .map(|(rel, chain)| {
                let last_ts = chain.iter().map(|s| s.timestamp).max().unwrap_or(0);
                (rel.clone(), chain.len(), last_ts)
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn restore_snapshot_with_stash(
        &self,
        path: &Path,
        snapshot_index: usize,
        expected_sha256: Option<&str>,
        stash_tool: &str,
        stash_description: &str,
    ) -> anyhow::Result<()> {
        self.ensure_loaded();
        let key = self.relative_key(path);
        let (hash, absent) = {
            let state = self.state.read();
            let chain = state
                .index
                .files
                .get(&key)
                .ok_or_else(|| anyhow::anyhow!("No edit history for: {key}"))?;
            let snap = chain
                .get(snapshot_index)
                .ok_or_else(|| anyhow::anyhow!("Snapshot index {snapshot_index} out of range"))?;
            (snap.sha256.clone(), snap.absent)
        };

        if let Some(expected) = expected_sha256 {
            if expected != hash {
                anyhow::bail!(
                    "Snapshot at index {snapshot_index} no longer matches the requested \
                     version; reload the history list and retry"
                );
            }
        }

        if let Err(e) = self.snapshot_before_write(path, stash_tool, stash_description) {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "restore_snapshot: failed to stash current state before revert"
            );
        }

        self.apply_pre_image(path, &hash, absent)
    }

    pub fn file_history_with_batches(
        &self,
        path: &Path,
    ) -> Vec<(FileSnapshot, Option<String>)> {
        self.ensure_loaded();
        let key = self.relative_key(path);
        let state = self.state.read();
        let Some(chain) = state.index.files.get(&key) else {
            return Vec::new();
        };
        chain
            .iter()
            .enumerate()
            .map(|(idx, snap)| {
                let batch = state
                    .timeline
                    .iter()
                    .find(|ev| ev.path == key && ev.snapshot_index == idx)
                    .and_then(|ev| ev.edit_batch_id.clone());
                (snap.clone(), batch)
            })
            .collect()
    }

    pub fn session_edited_files(&self, session_id: Option<&str>) -> Vec<SessionEditedFile> {
        self.ensure_loaded();
        let bucket = self.session_bucket(session_id);
        let state = self.state.read();
        let mut out: Vec<SessionEditedFile> = Vec::new();
        for (scoped, first_idx) in state.session_first_index.iter() {
            if !scoped_matches_bucket(scoped, &bucket) {
                continue;
            }
            let Some(key) = rel_from_scoped(scoped) else {
                continue;
            };
            let Some(chain) = state.index.files.get(key) else {
                continue;
            };
            let Some(snap) = chain.get(*first_idx) else {
                continue;
            };
            let mut batch_ids: Vec<String> = Vec::new();
            for ev in state
                .timeline
                .iter()
                .filter(|ev| ev.path == key && ev.snapshot_index >= *first_idx)
            {
                if let Some(id) = ev.edit_batch_id.as_deref() {
                    if !batch_ids.iter().any(|b| b == id) {
                        batch_ids.push(id.to_string());
                    }
                }
            }
            out.push(SessionEditedFile {
                rel_path: key.to_string(),
                first_snapshot_index: *first_idx,
                pre_image: snap.clone(),
                batch_ids,
            });
        }
        out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        out
    }

    pub fn session_first_index_for(
        &self,
        path: &Path,
        session_id: Option<&str>,
    ) -> Option<(usize, FileSnapshot)> {
        self.ensure_loaded();
        let key = self.relative_key(path);
        let bucket = self.session_bucket(session_id);
        let scoped = scoped_session_key(&bucket, &key);
        let state = self.state.read();
        let idx = *state.session_first_index.get(&scoped)?;
        let snap = state.index.files.get(&key)?.get(idx)?.clone();
        Some((idx, snap))
    }

    pub fn get_session_timeline(&self) -> Vec<EditEvent> {
        let state = self.state.read();
        state.timeline.clone()
    }

    pub fn stamp_latest_with_batch<P: AsRef<Path>>(
        &self,
        paths: impl IntoIterator<Item = P>,
        edit_batch_id: &str,
        precomputed_post_hashes: &HashMap<PathBuf, String>,
    ) {
        self.ensure_loaded();
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        let keys: Vec<String> = paths.iter().map(|p| self.relative_key(p)).collect();
        if keys.is_empty() {
            return;
        }
        let post_hashes: HashMap<String, String> = paths
            .iter()
            .zip(keys.iter())
            .filter_map(|(abs_path, key)| {
                if let Some(sha) = precomputed_post_hashes.get(abs_path) {
                    return Some((key.clone(), sha.clone()));
                }
                let abs = self.workspace_dir.join(key);
                std::fs::read(&abs).ok().map(|c| (key.clone(), Self::sha256(&c)))
            })
            .collect();

        let mut state = self.state.write();
        for key in &keys {
            let mut last_stamped: Option<usize> = None;
            for (pos, ev) in state.timeline.iter_mut().enumerate() {
                if &ev.path == key && ev.edit_batch_id.is_none() {
                    ev.edit_batch_id = Some(edit_batch_id.to_string());
                    last_stamped = Some(pos);
                }
            }
            if let Some(pos) = last_stamped {
                if let Some(ev) = state.timeline.get_mut(pos) {
                    ev.post_sha256 = post_hashes.get(key).cloned();
                }
            }
        }
        drop(state);
        self.save_index();
    }

    pub fn latest_batch_id_for(&self, path: &Path) -> Option<String> {
        self.ensure_loaded();
        let key = self.relative_key(path);
        let state = self.state.read();
        state
            .timeline
            .iter()
            .rev()
            .find(|ev| ev.path == key)
            .and_then(|ev| ev.edit_batch_id.clone())
    }

    pub fn snapshots_for_batch(
        &self,
        edit_batch_id: &str,
    ) -> Vec<(String, FileSnapshot)> {
        self.snapshots_for_batch_detailed(edit_batch_id)
            .into_iter()
            .map(|(path, snap, _)| (path, snap))
            .collect()
    }

    fn snapshots_for_batch_detailed(
        &self,
        edit_batch_id: &str,
    ) -> Vec<(String, FileSnapshot, Option<String>)> {
        self.ensure_loaded();
        let state = self.state.read();
        let mut out = Vec::new();
        for ev in state
            .timeline
            .iter()
            .filter(|e| e.edit_batch_id.as_deref() == Some(edit_batch_id))
        {
            if let Some(chain) = state.index.files.get(&ev.path) {
                if let Some(snap) = chain.get(ev.snapshot_index) {
                    out.push((ev.path.clone(), snap.clone(), ev.post_sha256.clone()));
                }
            }
        }
        out
    }

    pub fn revert_batch(&self, edit_batch_id: &str) -> anyhow::Result<Vec<String>> {
        let outcome = self.revert_batch_guarded(edit_batch_id, false)?;
        if !outcome.skipped_stale.is_empty() {
            tracing::warn!(
                batch = %edit_batch_id,
                skipped = ?outcome.skipped_stale,
                "revert_batch skipped files changed after the batch; not clobbering newer edits"
            );
        }
        Ok(outcome.reverted)
    }

    pub fn revert_batch_force(&self, edit_batch_id: &str) -> anyhow::Result<RevertOutcome> {
        self.revert_batch_guarded(edit_batch_id, true)
    }

    pub fn revert_batch_guarded(
        &self,
        edit_batch_id: &str,
        force: bool,
    ) -> anyhow::Result<RevertOutcome> {
        let snaps = self.snapshots_for_batch_detailed(edit_batch_id);
        let mut folded: Vec<(String, FileSnapshot, Option<String>)> = Vec::new();
        let mut slot_by_path: HashMap<String, usize> = HashMap::new();
        for (rel_path, snap, post_sha) in snaps {
            match slot_by_path.get(&rel_path) {
                Some(&slot) => {
                    if post_sha.is_some() {
                        folded[slot].2 = post_sha;
                    }
                }
                None => {
                    slot_by_path.insert(rel_path.clone(), folded.len());
                    folded.push((rel_path, snap, post_sha));
                }
            }
        }
        let mut outcome = RevertOutcome::default();
        for (rel_path, snap, post_sha) in folded {
            let abs = self.workspace_dir.join(&rel_path);

            let _write_guard = acquire_write_guard_blocking(&abs);

            if !force {
                if let Some(expected) = post_sha.as_deref() {
                    match std::fs::read(&abs) {
                        Ok(current) if Self::sha256(&current) != expected => {
                            outcome.skipped_stale.push(rel_path);
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            if snap.absent {
                match std::fs::remove_file(&abs) {
                    Ok(()) => outcome.reverted.push(rel_path),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        outcome.reverted.push(rel_path)
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "rewind",
                            path = %abs.display(),
                            error = %e,
                            "failed to delete created file during batch revert"
                        );
                    }
                }
                continue;
            }

            let content_path = self.storage_dir.join(&snap.sha256);
            let Ok(content) = std::fs::read(&content_path) else {
                continue;
            };
            if let Some(parent) = abs.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if crate::util::atomic_write(&abs, &content).is_ok() {
                crate::session::record_write_for_current_session(&abs);
                outcome.reverted.push(rel_path);
            }
        }
        if !outcome.reverted.is_empty() {
            crate::observability::session_write_mode_metrics::incr_checkpoint_rollback_via_edit_history();
        }
        Ok(outcome)
    }

    pub fn read_blob(&self, sha256: &str) -> anyhow::Result<Vec<u8>> {
        let path = self.storage_dir.join(sha256);
        std::fs::read(&path)
            .map_err(|e| anyhow::anyhow!("Edit-history blob missing ({sha256}): {e}"))
    }

    pub fn get_snapshot_content(
        &self,
        path: &Path,
        snapshot_index: usize,
    ) -> anyhow::Result<String> {
        self.ensure_loaded();
        let key = self.relative_key(path);

        let hash = {
            let state = self.state.read();
            let chain = state
                .index
                .files
                .get(&key)
                .ok_or_else(|| anyhow::anyhow!("No edit history for: {key}"))?;
            let snap = chain
                .get(snapshot_index)
                .ok_or_else(|| anyhow::anyhow!("Snapshot index {snapshot_index} out of range"))?;
            snap.sha256.clone()
        };

        let content_path = self.storage_dir.join(&hash);
        let content = std::fs::read_to_string(&content_path)
            .map_err(|e| anyhow::anyhow!("Snapshot content missing ({hash}): {e}"))?;

        Ok(content)
    }

    pub fn get_diff(&self, path: &Path, snapshot_index: usize) -> anyhow::Result<String> {
        let old = self.get_snapshot_content(path, snapshot_index)?;
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_dir.join(path)
        };
        let current =
            std::fs::read_to_string(&abs_path).unwrap_or_else(|_| "(file deleted)".to_string());

        let mut diff_lines = Vec::new();
        let key = self.relative_key(path);
        diff_lines.push(format!("--- a/{key}"));
        diff_lines.push(format!("+++ b/{key}"));

        let diff = similar::TextDiff::from_lines(&old, &current);
        for change in diff.iter_all_changes() {
            let value = change.value();
            let line = value.strip_suffix('\n').unwrap_or(value);
            match change.tag() {
                similar::ChangeTag::Equal => diff_lines.push(format!(" {line}")),
                similar::ChangeTag::Delete => diff_lines.push(format!("-{line}")),
                similar::ChangeTag::Insert => diff_lines.push(format!("+{line}")),
            }
        }

        Ok(diff_lines.join("\n"))
    }

    pub fn get_modified_files(&self) -> Vec<String> {
        self.ensure_loaded();
        let state = self.state.read();
        state.index.files.keys().cloned().collect()
    }

    pub fn cleanup_orphaned_content(&self) -> anyhow::Result<usize> {
        self.ensure_loaded();
        let state = self.state.read();

        let referenced: std::collections::HashSet<&str> = state
            .index
            .files
            .values()
            .flat_map(|chain| chain.iter().map(|s| s.sha256.as_str()))
            .collect();

        let mut removed = 0;
        if let Ok(entries) = std::fs::read_dir(&self.storage_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == INDEX_FILE {
                    continue;
                }
                if !referenced.contains(name_str.as_ref()) {
                    if std::fs::remove_file(entry.path()).is_ok() {
                        removed += 1;
                    }
                }
            }
        }

        Ok(removed)
    }
}
