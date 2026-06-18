// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

pub struct GroupDocStore {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ImportedDoc {
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub content_hash: String,
}

impl GroupDocStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn doc_dir(&self, group_id: &str, doc_id: &str) -> PathBuf {
        self.root
            .join(sanitize(group_id))
            .join("docs")
            .join(sanitize(doc_id))
    }

    pub fn content_path(&self, group_id: &str, doc_id: &str, name: &str) -> PathBuf {
        self.doc_dir(group_id, doc_id).join(sanitize_name(name))
    }

    pub fn is_available(&self, group_id: &str, doc_id: &str, name: &str) -> bool {
        self.content_path(group_id, doc_id, name).exists()
    }

    pub async fn import_local(
        &self,
        group_id: &str,
        doc_id: &str,
        source: &Path,
    ) -> Result<ImportedDoc> {
        if !source.exists() {
            return Err(anyhow!("source path does not exist"));
        }
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("invalid source name"))?;
        let doc_dir = self.doc_dir(group_id, doc_id);
        let dest = doc_dir.join(sanitize_name(&name));
        let src = source.to_path_buf();
        let imported = tokio::task::spawn_blocking(move || -> Result<ImportedDoc> {
            if doc_dir.exists() {
                std::fs::remove_dir_all(&doc_dir).ok();
            }
            std::fs::create_dir_all(&doc_dir)
                .with_context(|| format!("creating doc dir {}", doc_dir.display()))?;
            copy_path(&src, &dest)?;
            let (is_dir, size, content_hash) = hash_path(&dest)?;
            Ok(ImportedDoc {
                name,
                is_dir,
                size: i64::try_from(size).unwrap_or(i64::MAX),
                content_hash,
            })
        })
        .await
        .map_err(|e| anyhow!("import task failed: {e}"))??;
        Ok(imported)
    }

    pub async fn place_received(
        &self,
        group_id: &str,
        doc_id: &str,
        name: &str,
        source: &Path,
    ) -> Result<PathBuf> {
        let doc_dir = self.doc_dir(group_id, doc_id);
        let dest = doc_dir.join(sanitize_name(name));
        let src = source.to_path_buf();
        let placed = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
            if doc_dir.exists() {
                std::fs::remove_dir_all(&doc_dir).ok();
            }
            std::fs::create_dir_all(&doc_dir)
                .with_context(|| format!("creating doc dir {}", doc_dir.display()))?;
            if std::fs::rename(&src, &dest).is_err() {
                copy_path(&src, &dest)?;
                if src.is_dir() {
                    std::fs::remove_dir_all(&src).ok();
                } else {
                    std::fs::remove_file(&src).ok();
                }
            }
            Ok(dest)
        })
        .await
        .map_err(|e| anyhow!("place task failed: {e}"))??;
        Ok(placed)
    }

    pub fn remove_doc(&self, group_id: &str, doc_id: &str) {
        let doc_dir = self.doc_dir(group_id, doc_id);
        if doc_dir.exists() {
            let _ = std::fs::remove_dir_all(&doc_dir);
        }
    }
}

pub fn file_name_for(name: &str) -> String {
    sanitize_name(name)
}

pub fn copy_into(source: &Path, dest: &Path) -> Result<()> {
    copy_path(source, dest)
}

fn hash_path(path: &Path) -> Result<(bool, u64, String)> {
    let meta = std::fs::symlink_metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    if meta.is_dir() {
        let mut entries: Vec<(String, u64)> = Vec::new();
        collect_entries(path, path, &mut entries)?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        for (rel, len) in &entries {
            hasher.update(rel.as_bytes());
            hasher.update(b"\0");
            hasher.update(&len.to_le_bytes());
            total += len;
        }
        Ok((true, total, hex::encode(hasher.finalize())))
    } else {
        let mut file = std::fs::File::open(path)
            .with_context(|| format!("opening {} for hashing", path.display()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0u64;
        loop {
            use std::io::Read;
            let read = file.read(&mut buf)?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            total += read as u64;
        }
        Ok((false, total, hex::encode(hasher.finalize())))
    }
}

fn collect_entries(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            collect_entries(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().to_string());
            out.push((rel, meta.len()));
        }
    }
    Ok(())
}

fn copy_path(source: &Path, dest: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(source)
        .with_context(|| format!("reading metadata for {}", source.display()))?;
    if meta.is_dir() {
        std::fs::create_dir_all(dest)
            .with_context(|| format!("creating dir {}", dest.display()))?;
        for entry in std::fs::read_dir(source)
            .with_context(|| format!("reading dir {}", source.display()))?
        {
            let entry = entry?;
            let child_dest = dest.join(entry.file_name());
            copy_path(&entry.path(), &child_dest)?;
        }
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent {}", parent.display()))?;
        }
        std::fs::copy(source, dest)
            .with_context(|| format!("copying {} -> {}", source.display(), dest.display()))?;
    }
    Ok(())
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let cleaned: String = base
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}
