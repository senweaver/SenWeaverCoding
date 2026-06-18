// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::index::{BaselineIndex, BaselineRecord};
use parking_lot::RwLock;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_millis(1500);

struct Cache {
    signature: u64,
    files: BTreeSet<String>,
    index: BaselineIndex,
    scanned_at: Instant,
}

pub struct TemplateLibraryStore {
    root: PathBuf,
    cache: RwLock<Option<Cache>>,
}

impl TemplateLibraryStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("template-library"),
            cache: RwLock::new(None),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    fn ensure_fresh(&self) {
        {
            let guard = self.cache.read();
            if let Some(c) = guard.as_ref() {
                if c.scanned_at.elapsed() < CACHE_TTL {
                    return;
                }
            }
        }
        let mut guard = self.cache.write();
        if let Some(c) = guard.as_ref() {
            if c.scanned_at.elapsed() < CACHE_TTL {
                return;
            }
        }
        let signature = compute_signature(&self.root);
        let stale = match guard.as_ref() {
            Some(c) => c.signature != signature,
            None => true,
        };
        if stale {
            let files = scan_files(&self.root);
            let index = BaselineIndex::load(&self.index_path());
            *guard = Some(Cache {
                signature,
                files,
                index,
                scanned_at: Instant::now(),
            });
        } else if let Some(c) = guard.as_mut() {
            c.scanned_at = Instant::now();
        }
    }

    fn invalidate(&self) {
        *self.cache.write() = None;
    }

    pub fn read(&self, rel: &str) -> Option<String> {
        let rel = sanitize(rel)?;
        std::fs::read_to_string(self.root.join(&rel)).ok()
    }

    pub fn exists(&self, rel: &str) -> bool {
        match sanitize(rel) {
            Some(rel) => self.root.join(&rel).is_file(),
            None => false,
        }
    }

    pub fn list_files(&self, prefix: &str) -> Vec<String> {
        self.ensure_fresh();
        let prefix = prefix.trim_matches('/');
        let guard = self.cache.read();
        let Some(cache) = guard.as_ref() else {
            return Vec::new();
        };
        cache
            .files
            .iter()
            .filter(|p| {
                if prefix.is_empty() {
                    true
                } else {
                    p.as_str() == prefix || p.starts_with(&format!("{prefix}/"))
                }
            })
            .cloned()
            .collect()
    }

    pub fn child_dirs(&self, prefix: &str) -> Vec<String> {
        let prefix = prefix.trim_matches('/');
        let mut out: BTreeSet<String> = BTreeSet::new();
        for f in self.list_files(prefix) {
            let rest = if prefix.is_empty() {
                f.as_str()
            } else {
                match f.strip_prefix(&format!("{prefix}/")) {
                    Some(r) => r,
                    None => continue,
                }
            };
            if let Some((dir, _)) = rest.split_once('/') {
                out.insert(dir.to_string());
            }
        }
        out.into_iter().collect()
    }

    pub fn baseline_hash(&self, rel: &str) -> Option<String> {
        let rel = sanitize(rel)?;
        self.ensure_fresh();
        let guard = self.cache.read();
        guard
            .as_ref()
            .and_then(|c| c.index.baseline_hash(&rel).map(str::to_string))
    }

    pub fn save(&self, rel: &str, content: &str, baseline_hash: Option<String>) -> std::io::Result<()> {
        let rel = sanitize(rel).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid template path")
        })?;
        let target = self.root.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::util::atomic_write(&target, content.as_bytes())?;

        let mut index = BaselineIndex::load(&self.index_path());
        if let Some(hash) = baseline_hash {
            index.baselines.insert(
                rel.clone(),
                BaselineRecord {
                    hash,
                    edited_at: chrono::Utc::now().to_rfc3339(),
                },
            );
        } else {
            index.baselines.remove(&rel);
        }
        self.write_index(&index)?;
        self.invalidate();
        Ok(())
    }

    pub fn remove(&self, rel: &str) -> std::io::Result<()> {
        let rel = sanitize(rel).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid template path")
        })?;
        let target = self.root.join(&rel);
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        prune_empty_dirs(&self.root, target.parent());
        let mut index = BaselineIndex::load(&self.index_path());
        if index.baselines.remove(&rel).is_some() {
            self.write_index(&index)?;
        }
        self.invalidate();
        Ok(())
    }

    pub fn remove_entry(&self, dir_rel: &str) -> std::io::Result<()> {
        let dir_rel = sanitize(dir_rel).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid template path")
        })?;
        let target = self.root.join(&dir_rel);
        if target.is_dir() {
            std::fs::remove_dir_all(&target)?;
        } else if target.is_file() {
            std::fs::remove_file(&target)?;
        }
        prune_empty_dirs(&self.root, target.parent());
        let mut index = BaselineIndex::load(&self.index_path());
        let prefix = format!("{dir_rel}/");
        let before = index.baselines.len();
        index
            .baselines
            .retain(|k, _| k != &dir_rel && !k.starts_with(&prefix));
        if index.baselines.len() != before {
            self.write_index(&index)?;
        }
        self.invalidate();
        Ok(())
    }

    fn write_index(&self, index: &BaselineIndex) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let serialized = serde_json::to_vec_pretty(index)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        crate::util::atomic_write(&self.index_path(), &serialized)
    }
}

fn sanitize(rel: &str) -> Option<String> {
    let normalized = rel.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() {
        return None;
    }
    if normalized
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == ".." || seg.starts_with('.'))
    {
        return None;
    }
    Some(normalized.to_string())
}

fn scan_files(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if !root.is_dir() {
        return out;
    }
    walk_collect(root, root, &mut out);
    out
}

fn walk_collect(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name.ends_with(".tmp") {
            continue;
        }
        if path.is_dir() {
            walk_collect(root, &path, out);
        } else if path.is_file() {
            if name == "index.json" && path.parent() == Some(root) {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                out.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn compute_signature(root: &Path) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    let mut count: u64 = 0;
    fold_signature(root, &mut acc, &mut count);
    acc ^ count.wrapping_mul(0x100000001b3)
}

fn fold_signature(dir: &Path, acc: &mut u64, count: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".tmp") {
            continue;
        }
        if path.is_dir() {
            fold_signature(&path, acc, count);
        } else if let Ok(meta) = entry.metadata() {
            *count += 1;
            for b in name.as_bytes() {
                *acc = (*acc ^ (*b as u64)).wrapping_mul(0x100000001b3);
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            *acc = (*acc ^ mtime).wrapping_mul(0x100000001b3);
            *acc = (*acc ^ meta.len()).wrapping_mul(0x100000001b3);
        }
    }
}

fn prune_empty_dirs(root: &Path, mut dir: Option<&Path>) {
    while let Some(current) = dir {
        if current == root || !current.starts_with(root) {
            break;
        }
        match std::fs::read_dir(current) {
            Ok(mut it) => {
                if it.next().is_some() {
                    break;
                }
            }
            Err(_) => break,
        }
        if std::fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}
