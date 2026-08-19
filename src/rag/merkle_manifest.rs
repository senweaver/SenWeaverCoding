// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStamp {
    pub size: u64,
    pub mtime_ms: u64,
    pub sha256: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MerkleManifest {
    #[serde(default)]
    pub entries: HashMap<String, FileStamp>,
}

fn manifest_path(root: &Path) -> PathBuf {
    root.join(".sen").join("rag").join("merkle.json")
}

pub fn rel_key(root: &Path, abs: &Path) -> Option<String> {
    abs.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

pub fn file_meta_stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((meta.len(), mtime_ms))
}

impl MerkleManifest {
    #[must_use]
    pub fn load(root: &Path) -> Self {
        let path = manifest_path(root);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, root: &Path) {
        let path = manifest_path(root);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(serialized) = serde_json::to_vec(self) {
            let _ = crate::util::atomic_write(&path, &serialized);
        }
    }

    #[must_use]
    pub fn is_unchanged_fast(&self, root: &Path, abs: &Path) -> bool {
        let Some(key) = rel_key(root, abs) else {
            return false;
        };
        let Some(stamp) = self.entries.get(&key) else {
            return false;
        };
        match file_meta_stamp(abs) {
            Some((size, mtime_ms)) => size == stamp.size && mtime_ms == stamp.mtime_ms,
            None => false,
        }
    }

    #[must_use]
    pub fn sha_matches(&self, root: &Path, abs: &Path, sha256: &str) -> bool {
        rel_key(root, abs)
            .and_then(|key| self.entries.get(&key).map(|s| s.sha256 == sha256))
            .unwrap_or(false)
    }

    pub fn record_with_sha(&mut self, root: &Path, abs: &Path, sha256: String) {
        let Some(key) = rel_key(root, abs) else {
            return;
        };
        let Some((size, mtime_ms)) = file_meta_stamp(abs) else {
            return;
        };
        self.entries.insert(
            key,
            FileStamp {
                size,
                mtime_ms,
                sha256,
            },
        );
    }

    pub fn remove(&mut self, root: &Path, abs: &Path) {
        if let Some(key) = rel_key(root, abs) {
            self.entries.remove(&key);
        }
    }

    pub fn retain_keys(&mut self, keep: &HashSet<String>) {
        self.entries.retain(|k, _| keep.contains(k));
    }
}
