// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: u32,
    pub snippet: String,
}

pub trait IncrementalIndex: Send + Sync {

    fn reindex_file(&self, path: &Path) -> std::io::Result<()>;

    fn remove_file(&self, path: &Path) -> std::io::Result<()>;

    fn search(&self, query: &str, limit: usize) -> std::io::Result<Vec<SearchHit>>;

    fn size_on_disk_bytes(&self) -> u64 {
        0
    }
}

pub mod heuristic {

    use super::{IncrementalIndex, SearchHit};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::RwLock;

    pub struct Search {
        root: PathBuf,

        max_file_bytes: u64,

        last_hits: RwLock<Vec<SearchHit>>,
    }

    impl Search {
        pub fn new<P: Into<PathBuf>>(root: P) -> Self {
            Self {
                root: root.into(),
                max_file_bytes: 4 * 1024 * 1024,
                last_hits: RwLock::new(Vec::new()),
            }
        }

        pub fn with_max_file_bytes(mut self, bytes: u64) -> Self {
            self.max_file_bytes = bytes;
            self
        }

        fn walk(&self, mut f: impl FnMut(PathBuf)) {
            let mut stack = vec![self.root.clone()];
            while let Some(dir) = stack.pop() {
                let entries = match std::fs::read_dir(&dir) {
                    Ok(it) => it,
                    Err(_) => continue,
                };
                for entry in entries.flatten() {
                    let path = entry.path();

                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if matches!(
                            name,
                            ".git" | "target" | "node_modules" | ".venv" | "__pycache__"
                        ) {
                            continue;
                        }
                    }
                    match entry.file_type() {
                        Ok(ft) if ft.is_dir() => stack.push(path),
                        Ok(ft) if ft.is_file() => f(path),
                        _ => {}
                    }
                }
            }
        }
    }

    impl IncrementalIndex for Search {
        fn reindex_file(&self, _path: &Path) -> io::Result<()> {

            Ok(())
        }

        fn remove_file(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn search(&self, query: &str, limit: usize) -> io::Result<Vec<SearchHit>> {
            const SEARCH_TIMEOUT_SECS: u64 = 10;

            if query.is_empty() {
                return Ok(Vec::new());
            }
            let mut hits = Vec::new();
            let q_lower = query.to_lowercase();
            let mut aborted = false;
            let deadline = std::time::Instant::now()
                + std::time::Duration::from_secs(SEARCH_TIMEOUT_SECS);

            self.walk(|file| {
                if aborted {
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    aborted = true;
                    return;
                }
                let meta = match std::fs::metadata(&file) {
                    Ok(m) => m,
                    Err(_) => return,
                };
                if meta.len() > self.max_file_bytes {
                    return;
                }
                let content = match std::fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                for (idx, raw) in content.lines().enumerate() {
                    if raw.to_lowercase().contains(&q_lower) {
                        hits.push(SearchHit {
                            path: file.clone(),
                            line: idx as u32 + 1,
                            snippet: raw.trim().to_string(),
                        });
                        if hits.len() >= limit {
                            aborted = true;
                            break;
                        }
                    }
                }
            });

            if let Ok(mut cache) = self.last_hits.write() {
                *cache = hits.clone();
            }
            Ok(hits)
        }
    }

}
