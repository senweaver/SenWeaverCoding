// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Code search — full-text index with opt-in tantivy back-end.
//!
//! the code-intelligence story requires a fast
//! substring/token search over the workspace (think "find every
//! definition of `fn parse_`").  Full-blown BM25 indexing via
//! [`tantivy`] is overkill for tiny workspaces and too heavy a
//! compile-time cost to ship unconditionally, so the module exposes:
//!
//! - An always-available **heuristic** scanner ([`heuristic::Search`])
//!   that walks the directory tree and runs a regex / substring match
//!   per line.  Zero extra dependencies; perfectly adequate for
//!   workspaces under a few tens of megabytes.
//! - A **tantivy-backed** index ([`tantivy_backend::TantivyIndex`])
//!   behind `#[cfg(feature = "code-search")]` for larger trees, with
//!   an [`IncrementalIndex`] trait so call-sites can swap back-ends
//!   without changing their code.
//!
//! Prometheus hook: whenever the tantivy index is rebuilt it reports
//! the on-disk byte size through
//! [`crate::observability::prometheus::PrometheusObserver::set_fulltext_index_size_bytes`].

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
            if query.is_empty() {
                return Ok(Vec::new());
            }
            let mut hits = Vec::new();
            let q_lower = query.to_lowercase();
            let mut aborted = false;

            self.walk(|file| {
                if aborted {
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

#[cfg(feature = "code-search")]
pub mod tantivy_backend {

    use super::{IncrementalIndex, SearchHit};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::RwLock;

    use tantivy::collector::TopDocs;
    use tantivy::query::QueryParser;
    use tantivy::schema::{STORED, Schema, TEXT};
    use tantivy::{Index, IndexReader, IndexWriter, doc};

    pub struct TantivyIndex {
        root: PathBuf,
        index: Index,
        reader: IndexReader,
        writer: RwLock<IndexWriter>,
        schema: Schema,
        index_dir: PathBuf,
    }

    impl TantivyIndex {

        pub fn open(root: PathBuf, index_dir: PathBuf) -> io::Result<Self> {
            std::fs::create_dir_all(&index_dir)?;
            let mut builder = Schema::builder();
            builder.add_text_field("path", STORED | TEXT);
            builder.add_u64_field("line", STORED);
            builder.add_text_field("body", TEXT | STORED);
            let schema = builder.build();
            let index = Index::open_or_create(
                tantivy::directory::MmapDirectory::open(&index_dir).map_err(to_io)?,
                schema.clone(),
            )
            .map_err(to_io)?;
            let writer = index.writer(50_000_000).map_err(to_io)?;
            let reader = index.reader().map_err(to_io)?;
            Ok(Self {
                root,
                index,
                reader,
                writer: RwLock::new(writer),
                schema,
                index_dir,
            })
        }

        pub fn disk_footprint(&self) -> u64 {
            let mut total = 0u64;
            let mut stack = vec![self.index_dir.clone()];
            while let Some(dir) = stack.pop() {
                let entries = match std::fs::read_dir(&dir) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                for e in entries.flatten() {
                    let path = e.path();
                    match e.file_type() {
                        Ok(ft) if ft.is_dir() => stack.push(path),
                        Ok(ft) if ft.is_file() => {
                            if let Ok(m) = path.metadata() {
                                total += m.len();
                            }
                        }
                        _ => {}
                    }
                }
            }
            total
        }
    }

    fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
        io::Error::new(io::ErrorKind::Other, e.to_string())
    }

    impl IncrementalIndex for TantivyIndex {
        fn reindex_file(&self, path: &Path) -> io::Result<()> {
            let content = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            let path_field = self.schema.get_field("path").unwrap();
            let line_field = self.schema.get_field("line").unwrap();
            let body_field = self.schema.get_field("body").unwrap();

            let mut writer = self
                .writer
                .write()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "writer lock poisoned"))?;

            let _ = writer.delete_term(tantivy::Term::from_field_text(
                path_field,
                &path.display().to_string(),
            ));
            for (idx, raw) in content.lines().enumerate() {
                writer
                    .add_document(doc!(
                        path_field => path.display().to_string(),
                        line_field => (idx as u64) + 1,
                        body_field => raw.to_string(),
                    ))
                    .map_err(to_io)?;
            }
            writer.commit().map_err(to_io)?;
            self.reader.reload().map_err(to_io)?;

            if let Some(svc) = crate::services::try_get_services() {
                if let Some(obs) = &svc.prometheus {
                    obs.set_fulltext_index_size_bytes(self.disk_footprint() as i64);
                }
            }
            Ok(())
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            let path_field = self.schema.get_field("path").unwrap();
            let mut writer = self
                .writer
                .write()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "writer lock poisoned"))?;
            let _ = writer.delete_term(tantivy::Term::from_field_text(
                path_field,
                &path.display().to_string(),
            ));
            writer.commit().map_err(to_io)?;
            self.reader.reload().map_err(to_io)?;
            Ok(())
        }

        fn search(&self, query: &str, limit: usize) -> io::Result<Vec<SearchHit>> {
            if query.is_empty() {
                return Ok(Vec::new());
            }
            let searcher = self.reader.searcher();
            let body_field = self.schema.get_field("body").unwrap();
            let path_field = self.schema.get_field("path").unwrap();
            let line_field = self.schema.get_field("line").unwrap();
            let parser = QueryParser::for_index(&self.index, vec![body_field]);
            let parsed = parser.parse_query(query).map_err(to_io)?;
            let docs = searcher
                .search(&parsed, &TopDocs::with_limit(limit))
                .map_err(to_io)?;
            let mut hits = Vec::new();
            for (_score, addr) in docs {
                let doc: tantivy::TantivyDocument = searcher.doc(addr).map_err(to_io)?;
                let p = doc
                    .get_first(path_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let line = doc
                    .get_first(line_field)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let snippet = doc
                    .get_first(body_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                hits.push(SearchHit {
                    path: PathBuf::from(p),
                    line,
                    snippet,
                });
            }
            Ok(hits)
        }

        fn size_on_disk_bytes(&self) -> u64 {
            self.disk_footprint()
        }
    }
}
