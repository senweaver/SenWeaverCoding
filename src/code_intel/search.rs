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
    use std::sync::OnceLock;

    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "that", "this", "from", "into", "when", "where", "what",
        "how", "why", "are", "was", "were", "has", "have", "had", "can", "could", "should",
        "would", "will", "not", "all", "any", "one", "two", "use", "used", "using", "does",
        "please", "fix", "add", "make", "need", "want", "then", "than", "them", "there",
        "here", "you", "your", "our", "its", "his", "her", "let", "get", "set", "run", "see",
    ];

    const MAX_QUERY_TERMS: usize = 8;
    const SKIP_DIRS: &[&str] = &[
        ".git",
        "target",
        "node_modules",
        ".venv",
        "venv",
        "__pycache__",
        "dist",
        "build",
        "vendor",
        ".next",
        "coverage",
        ".idea",
        ".vscode",
        "out",
    ];

    pub struct Search {
        root: PathBuf,

        max_file_bytes: u64,

        ignore_set: OnceLock<Option<globset::GlobSet>>,
    }

    impl Search {
        pub fn new<P: Into<PathBuf>>(root: P) -> Self {
            Self {
                root: root.into(),
                max_file_bytes: 4 * 1024 * 1024,
                ignore_set: OnceLock::new(),
            }
        }

        pub fn with_max_file_bytes(mut self, bytes: u64) -> Self {
            self.max_file_bytes = bytes;
            self
        }

        fn ignore_set(&self) -> Option<&globset::GlobSet> {
            self.ignore_set
                .get_or_init(|| build_gitignore_set(&self.root))
                .as_ref()
        }

        fn is_ignored(&self, path: &Path) -> bool {
            let Some(set) = self.ignore_set() else {
                return false;
            };
            let rel = path.strip_prefix(&self.root).unwrap_or(path);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            set.is_match(rel_str.as_str())
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
                        if name.starts_with('.') && name != "." {
                            if matches!(entry.file_type(), Ok(ft) if ft.is_dir()) {
                                continue;
                            }
                        }
                        if SKIP_DIRS.contains(&name) {
                            continue;
                        }
                    }
                    if self.is_ignored(&path) {
                        continue;
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

    fn build_gitignore_set(root: &Path) -> Option<globset::GlobSet> {
        let body = std::fs::read_to_string(root.join(".gitignore")).ok()?;
        let mut builder = globset::GlobSetBuilder::new();
        let mut added = 0usize;
        for raw in body.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let trimmed = line.trim_start_matches('/');
            let base = trimmed.trim_end_matches('/');
            if base.is_empty() {
                continue;
            }
            let patterns = if line.ends_with('/') || !base.contains('.') {
                vec![
                    base.to_string(),
                    format!("{base}/**"),
                    format!("**/{base}"),
                    format!("**/{base}/**"),
                ]
            } else {
                vec![base.to_string(), format!("**/{base}")]
            };
            for p in patterns {
                if let Ok(glob) = globset::GlobBuilder::new(&p)
                    .literal_separator(false)
                    .build()
                {
                    builder.add(glob);
                    added += 1;
                }
            }
            if added > 512 {
                break;
            }
        }
        builder.build().ok()
    }

    fn tokenize_query(query: &str) -> Vec<String> {
        let mut terms: Vec<String> = Vec::new();
        for run in query
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|s| !s.is_empty())
        {
            let lower = run.to_lowercase();
            let is_ascii = lower.is_ascii();
            if is_ascii && lower.len() < 3 {
                continue;
            }
            if is_ascii && STOPWORDS.contains(&lower.as_str()) {
                continue;
            }
            if !terms.contains(&lower) {
                terms.push(lower);
            }
            if terms.len() >= MAX_QUERY_TERMS {
                break;
            }
        }
        terms
    }

    struct ScoredHit {
        hit: SearchHit,
        score: u32,
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

            let query = query.trim();
            if query.is_empty() {
                return Ok(Vec::new());
            }
            let q_lower = query.to_lowercase();
            let terms = tokenize_query(query);
            if terms.is_empty() && q_lower.is_empty() {
                return Ok(Vec::new());
            }
            // Multi-term natural-language queries must match a meaningful
            // share of terms; single identifiers keep exact behavior.
            let min_terms: u32 = if terms.len() >= 4 {
                (terms.len() as u32).div_ceil(2)
            } else if terms.is_empty() {
                0
            } else {
                1
            };

            let mut scored: Vec<ScoredHit> = Vec::new();
            let keep = limit.saturating_mul(4).max(32);
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
                let mut file_hits = 0usize;
                for (idx, raw) in content.lines().enumerate() {
                    let line_lower = raw.to_lowercase();
                    let mut score: u32 = 0;
                    for term in &terms {
                        if line_lower.contains(term.as_str()) {
                            score += 1;
                        }
                    }
                    // Whole-phrase match keeps the old exact-substring
                    // behavior as the strongest signal.
                    if !q_lower.is_empty() && line_lower.contains(&q_lower) {
                        score += 3;
                    }
                    if score < min_terms.max(1) {
                        continue;
                    }
                    scored.push(ScoredHit {
                        hit: SearchHit {
                            path: file.clone(),
                            line: idx as u32 + 1,
                            snippet: raw.trim().to_string(),
                        },
                        score,
                    });
                    file_hits += 1;
                    if file_hits >= 6 {
                        break;
                    }
                }
                if scored.len() >= keep.saturating_mul(4) {
                    scored.sort_by(|a, b| b.score.cmp(&a.score));
                    scored.truncate(keep);
                }
            });

            scored.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| a.hit.snippet.len().cmp(&b.hit.snippet.len()))
            });
            let hits: Vec<SearchHit> = scored
                .into_iter()
                .take(limit)
                .map(|s| s.hit)
                .collect();
            Ok(hits)
        }
    }

}
