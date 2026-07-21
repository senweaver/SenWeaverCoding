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

const MAX_SNAPSHOTS_PER_FILE: usize = 20;
const MAX_TIMELINE_EVENTS: usize = 4096;
const HISTORY_DIR_NAME: &str = ".sen/edit_history";
const INDEX_FILE: &str = "index.json";

static SHARED_HISTORIES: once_cell::sync::Lazy<
    parking_lot::Mutex<HashMap<PathBuf, Arc<EditHistory>>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub sha256: String,
    pub timestamp: u64,
    pub tool_name: String,
    pub description: String,
    pub byte_size: usize,
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

    /// SHA-256 of the file *after* the batch landed on disk, captured when the
    /// batch id is stamped. `revert_batch` compares this against the current disk
    /// content and refuses to clobber a file that changed after the batch (e.g.
    /// a concurrent agent edit while a review panel is open).
    #[serde(default)]
    pub post_sha256: Option<String>,
}

/// Result of reverting an edit batch: which files were restored, and which were
/// left untouched because they changed after the batch (and reverting would have
/// silently discarded newer work).
#[derive(Debug, Clone, Default)]
pub struct RevertOutcome {
    pub reverted: Vec<String>,
    pub skipped_stale: Vec<String>,
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
        if !path.exists() {
            return Ok(());
        }

        self.ensure_loaded();

        let content = std::fs::read(path)?;
        let hash = Self::sha256(&content);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let content_path = self.storage_dir.join(&hash);
        if !content_path.exists() {
            std::fs::write(&content_path, &content)?;
        }

        let key = self.relative_key(path);
        let snapshot = FileSnapshot {
            sha256: hash,
            timestamp: now,
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            byte_size: content.len(),
        };

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
                if last.sha256 == snapshot.sha256 {
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
            }

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
        let content = std::fs::read(&content_path)
            .map_err(|e| anyhow::anyhow!("Snapshot content missing ({hash}): {e}"))?;

        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_dir.join(path)
        };
        crate::util::atomic_write(&abs_path, &content)?;

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
        let mut reverted = Vec::new();

        let paths_to_revert: Vec<(String, String)> = {
            let state = self.state.read();
            let session_paths: Vec<String> = state
                .timeline
                .iter()
                .map(|e| e.path.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            session_paths
                .into_iter()
                .filter_map(|key| {
                    state
                        .index
                        .files
                        .get(&key)
                        .and_then(|chain| chain.first())
                        .map(|snap| (key, snap.sha256.clone()))
                })
                .collect()
        };

        for (key, hash) in paths_to_revert {
            let content_path = self.storage_dir.join(&hash);
            if let Ok(content) = std::fs::read(&content_path) {
                let abs_path = self.workspace_dir.join(&key);
                if crate::util::atomic_write(&abs_path, &content).is_ok() {
                    reverted.push(key);
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
        // Prefer the exact post-image sha the applier already computed (keyed to
        // this batch's own write); only re-read from disk as a fallback, which
        // avoids a TOCTOU where a concurrent external write would poison the
        // revert freshness guard.
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
            if let Some(ev) = state
                .timeline
                .iter_mut()
                .rev()
                .find(|e| &e.path == key && e.edit_batch_id.is_none())
            {
                ev.edit_batch_id = Some(edit_batch_id.to_string());
                ev.post_sha256 = post_hashes.get(key).cloned();
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

    /// Revert a batch, refusing any file whose current on-disk content no longer
    /// matches the post-image captured when the batch was stamped (i.e. it was
    /// changed after the batch). This prevents a review-panel revert from
    /// silently clobbering a concurrent agent/user edit.
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

    /// Revert a batch unconditionally, ignoring the freshness guard. Only for
    /// explicit user-confirmed force-revert flows.
    pub fn revert_batch_force(&self, edit_batch_id: &str) -> anyhow::Result<RevertOutcome> {
        self.revert_batch_guarded(edit_batch_id, true)
    }

    pub fn revert_batch_guarded(
        &self,
        edit_batch_id: &str,
        force: bool,
    ) -> anyhow::Result<RevertOutcome> {
        let snaps = self.snapshots_for_batch_detailed(edit_batch_id);
        let mut outcome = RevertOutcome::default();
        for (rel_path, snap, post_sha) in snaps {
            let abs = self.workspace_dir.join(&rel_path);

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

            let content_path = self.storage_dir.join(&snap.sha256);
            let Ok(content) = std::fs::read(&content_path) else {
                continue;
            };
            if let Some(parent) = abs.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if crate::util::atomic_write(&abs, &content).is_ok() {
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

        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = current.lines().collect();

        let max = old_lines.len().max(new_lines.len());
        for i in 0..max {
            match (old_lines.get(i), new_lines.get(i)) {
                (Some(o), Some(n)) if o == n => {
                    diff_lines.push(format!(" {o}"));
                }
                (Some(o), Some(n)) => {
                    diff_lines.push(format!("-{o}"));
                    diff_lines.push(format!("+{n}"));
                }
                (Some(o), None) => {
                    diff_lines.push(format!("-{o}"));
                }
                (None, Some(n)) => {
                    diff_lines.push(format!("+{n}"));
                }
                (None, None) => {}
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
