// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use sha2::{Digest, Sha256};

const BLOB_ID_LEN: usize = 16;
const MAX_BLOBS: usize = 512;
const EVICT_TO: usize = 384;
const MAX_BLOB_BYTES: usize = 8 * 1024 * 1024;
const DEFERRED_WRITE_MIN_BYTES: usize = 256 * 1024;
const EVICT_CHECK_EVERY: u32 = 32;

static PUTS_SINCE_EVICT_CHECK: AtomicU32 = AtomicU32::new(0);

fn maybe_evict(dir: &std::path::Path) {
    let n = PUTS_SINCE_EVICT_CHECK.fetch_add(1, Ordering::Relaxed);
    if n % EVICT_CHECK_EVERY == 0 {
        evict_if_needed(dir);
    }
}

pub fn store_dir() -> PathBuf {
    directories::UserDirs::new()
        .map_or_else(
            || PathBuf::from(".senweavercoding"),
            |dirs| dirs.home_dir().join(".senweavercoding"),
        )
        .join("history_blobs")
}

fn is_valid_id(id: &str) -> bool {
    id.len() == BLOB_ID_LEN && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn put(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > MAX_BLOB_BYTES {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    let id = digest[..BLOB_ID_LEN].to_string();
    let dir = store_dir();
    let path = dir.join(format!("{id}.txt"));
    if path.exists() {
        touch(&path);
        return Some(id);
    }
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    if crate::util::atomic_write(&path, text.as_bytes()).is_err() {
        return None;
    }
    maybe_evict(&dir);
    Some(id)
}

pub fn put_offloaded(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > MAX_BLOB_BYTES {
        return None;
    }
    if text.len() < DEFERRED_WRITE_MIN_BYTES
        || tokio::runtime::Handle::try_current().is_err()
    {
        return put(text);
    }
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    let id = digest[..BLOB_ID_LEN].to_string();
    let owned = text.to_string();
    let id_for_task = id.clone();
    crate::runtime::spawn_supervised("history.blob_store.write", async move {
        let _ = tokio::task::spawn_blocking(move || {
            let dir = store_dir();
            let path = dir.join(format!("{id_for_task}.txt"));
            if path.exists() {
                touch(&path);
                return;
            }
            if std::fs::create_dir_all(&dir).is_err() {
                return;
            }
            let _ = crate::util::atomic_write(&path, owned.as_bytes());
            maybe_evict(&dir);
        })
        .await;
    });
    Some(id)
}

pub fn get(id: &str) -> Option<String> {
    if !is_valid_id(id) {
        return None;
    }
    let path = store_dir().join(format!("{id}.txt"));
    let content = std::fs::read_to_string(&path).ok()?;
    touch(&path);
    Some(content)
}

fn touch(path: &std::path::Path) {
    let now = std::fs::FileTimes::new()
        .set_accessed(std::time::SystemTime::now())
        .set_modified(std::time::SystemTime::now());
    if let Ok(file) = std::fs::OpenOptions::new().append(true).open(path) {
        let _ = file.set_times(now);
    }
}

fn evict_if_needed(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().is_some_and(|ext| ext == "txt") {
                let mtime = e.metadata().ok()?.modified().ok()?;
                Some((mtime, path))
            } else {
                None
            }
        })
        .collect();
    if files.len() <= MAX_BLOBS {
        return;
    }
    files.sort_by_key(|(mtime, _)| *mtime);
    let excess = files.len().saturating_sub(EVICT_TO);
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}
