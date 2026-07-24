// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: u32,
    pub snippet: String,
    pub end_line: Option<u32>,
}

pub trait IncrementalIndex: Send + Sync {

    fn reindex_file(&self, path: &Path) -> std::io::Result<()>;

    fn remove_file(&self, path: &Path) -> std::io::Result<()>;

    fn search(&self, query: &str, limit: usize) -> std::io::Result<Vec<SearchHit>>;

    fn search_with_focus(
        &self,
        query: &str,
        limit: usize,
        _focus: &[PathBuf],
    ) -> std::io::Result<Vec<SearchHit>> {
        self.search(query, limit)
    }

    fn size_on_disk_bytes(&self) -> u64 {
        0
    }

    fn mark_walk_fresh(&self) {}
}

pub fn build_gitignore_set(root: &Path) -> Option<globset::GlobSet> {
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

pub fn path_is_gitignored(set: &globset::GlobSet, root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    set.is_match(rel_str.as_str())
}

pub mod heuristic {

    use super::{IncrementalIndex, SearchHit};
    use std::collections::{HashMap, HashSet};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant, SystemTime};

    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "that", "this", "from", "into", "when", "where", "what",
        "how", "why", "are", "was", "were", "has", "have", "had", "can", "could", "should",
        "would", "will", "not", "all", "any", "one", "two", "use", "used", "using", "does",
        "please", "fix", "add", "make", "need", "want", "then", "than", "them", "there",
        "here", "you", "your", "our", "its", "his", "her", "let", "get", "set", "run", "see",
    ];

    const STOPWORDS_CJK: &[&str] = &[
        "帮我", "一下", "如何", "怎么", "什么", "修改", "实现", "添加", "删除", "文件",
        "代码", "这个", "那个", "可以", "需要", "问题", "为什", "然后", "现在", "一个",
        "不要", "使用", "请问", "麻烦", "所有", "进行", "或者", "以及", "但是", "如果",
    ];

    const MAX_QUERY_TERMS: usize = 8;
    const MAX_CANDIDATE_FILES: usize = 400;
    const WALK_TTL: Duration = Duration::from_secs(120);
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

        state: parking_lot::Mutex<IndexState>,
    }

    #[derive(Default)]
    struct IndexState {
        paths: Vec<PathBuf>,
        ids: HashMap<PathBuf, u32>,
        files: HashMap<u32, IndexedFile>,
        postings: HashMap<String, HashSet<u32>>,
        trigram_postings: HashMap<[u8; 3], HashSet<u32>>,
        last_walk: Option<Instant>,
    }

    fn for_each_ascii_trigram(token: &str, mut f: impl FnMut([u8; 3])) {
        if !token.is_ascii() || token.len() < 3 {
            return;
        }
        for w in token.as_bytes().windows(3) {
            f([w[0], w[1], w[2]]);
        }
    }

    struct IndexedFile {
        mtime_secs: u64,
        lines: Arc<Vec<String>>,
        tokens: Vec<String>,
    }

    fn is_cjk(c: char) -> bool {
        matches!(c,
            '\u{4E00}'..='\u{9FFF}'
                | '\u{3400}'..='\u{4DBF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{3040}'..='\u{30FF}'
                | '\u{AC00}'..='\u{D7AF}'
        )
    }

    fn file_mtime_secs(meta: &std::fs::Metadata) -> u64 {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn tokenize_content(lines: &[String]) -> Vec<String> {
        let mut tokens: HashSet<String> = HashSet::new();
        for line in lines {
            let mut ascii_run = String::new();
            let mut cjk_prev: Option<char> = None;
            for c in line.chars() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    ascii_run.push(c.to_ascii_lowercase());
                    cjk_prev = None;
                    continue;
                }
                if !ascii_run.is_empty() {
                    if ascii_run.len() >= 3 {
                        tokens.insert(std::mem::take(&mut ascii_run));
                    } else {
                        ascii_run.clear();
                    }
                }
                if is_cjk(c) {
                    if let Some(prev) = cjk_prev {
                        let mut gram = String::with_capacity(8);
                        gram.push(prev);
                        gram.push(c);
                        tokens.insert(gram);
                    }
                    cjk_prev = Some(c);
                } else {
                    cjk_prev = None;
                }
            }
            if ascii_run.len() >= 3 {
                tokens.insert(ascii_run);
            }
        }
        tokens.into_iter().collect()
    }

    #[derive(Debug)]
    enum QueryTerm {
        Ascii(String),
        Cjk { run: String, grams: Vec<String> },
    }

    impl QueryTerm {
        fn line_needle(&self) -> &str {
            match self {
                QueryTerm::Ascii(t) => t,
                QueryTerm::Cjk { run, .. } => run,
            }
        }
    }

    fn tokenize_query(query: &str) -> Vec<QueryTerm> {
        let mut terms: Vec<QueryTerm> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for run in query
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|s| !s.is_empty())
        {
            if terms.len() >= MAX_QUERY_TERMS {
                break;
            }
            if run.chars().any(is_cjk) {
                let chars: Vec<char> = run.chars().filter(|c| is_cjk(*c)).collect();
                if chars.len() < 2 {
                    continue;
                }
                let cjk_run: String = chars.iter().collect();
                if STOPWORDS_CJK.contains(&cjk_run.as_str()) {
                    continue;
                }
                if !seen.insert(cjk_run.clone()) {
                    continue;
                }
                let grams: Vec<String> = chars
                    .windows(2)
                    .map(|w| w.iter().collect::<String>())
                    .collect();
                terms.push(QueryTerm::Cjk {
                    run: cjk_run,
                    grams,
                });
                continue;
            }
            let lower = run.to_lowercase();
            if !lower.is_ascii() || lower.len() < 3 {
                continue;
            }
            if STOPWORDS.contains(&lower.as_str()) {
                continue;
            }
            if seen.insert(lower.clone()) {
                terms.push(QueryTerm::Ascii(lower));
            }
        }
        terms
    }

    struct ScoredHit {
        hit: SearchHit,
        score: u32,
    }

    impl Search {
        pub fn new<P: Into<PathBuf>>(root: P) -> Self {
            Self {
                root: root.into(),
                max_file_bytes: 4 * 1024 * 1024,
                ignore_set: OnceLock::new(),
                state: parking_lot::Mutex::new(IndexState::default()),
            }
        }

        pub fn with_max_file_bytes(mut self, bytes: u64) -> Self {
            self.max_file_bytes = bytes;
            self
        }

        fn ignore_set(&self) -> Option<&globset::GlobSet> {
            self.ignore_set
                .get_or_init(|| super::build_gitignore_set(&self.root))
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

        fn intern_id(state: &mut IndexState, path: &Path) -> u32 {
            if let Some(id) = state.ids.get(path) {
                return *id;
            }
            let id = state.paths.len() as u32;
            state.paths.push(path.to_path_buf());
            state.ids.insert(path.to_path_buf(), id);
            id
        }

        fn unindex(state: &mut IndexState, id: u32) {
            if let Some(old) = state.files.remove(&id) {
                for token in &old.tokens {
                    if let Some(set) = state.postings.get_mut(token) {
                        set.remove(&id);
                        if set.is_empty() {
                            state.postings.remove(token);
                        }
                    }
                    for_each_ascii_trigram(token, |key| {
                        if let Some(set) = state.trigram_postings.get_mut(&key) {
                            set.remove(&id);
                            if set.is_empty() {
                                state.trigram_postings.remove(&key);
                            }
                        }
                    });
                }
            }
        }

        fn index_content(state: &mut IndexState, id: u32, mtime_secs: u64, lines: Vec<String>) {
            Self::unindex(state, id);
            let tokens = tokenize_content(&lines);
            for token in &tokens {
                state
                    .postings
                    .entry(token.clone())
                    .or_default()
                    .insert(id);
                for_each_ascii_trigram(token, |key| {
                    state
                        .trigram_postings
                        .entry(key)
                        .or_default()
                        .insert(id);
                });
            }
            state.files.insert(
                id,
                IndexedFile {
                    mtime_secs,
                    lines: Arc::new(lines),
                    tokens,
                },
            );
        }

        fn reindex_from_disk(&self, state: &mut IndexState, path: &Path) -> io::Result<()> {
            let meta = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    if let Some(id) = state.ids.get(path).copied() {
                        Self::unindex(state, id);
                    }
                    return Ok(());
                }
                Err(e) => return Err(e),
            };
            if meta.len() > self.max_file_bytes {
                if let Some(id) = state.ids.get(path).copied() {
                    Self::unindex(state, id);
                }
                return Ok(());
            }
            let mtime_secs = file_mtime_secs(&meta);
            if let Some(id) = state.ids.get(path).copied() {
                if let Some(existing) = state.files.get(&id) {
                    if existing.mtime_secs == mtime_secs {
                        return Ok(());
                    }
                }
            }
            let content = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => {
                    if let Some(id) = state.ids.get(path).copied() {
                        Self::unindex(state, id);
                    }
                    return Ok(());
                }
            };
            let lines: Vec<String> = content.lines().map(str::to_string).collect();
            let id = Self::intern_id(state, path);
            Self::index_content(state, id, mtime_secs, lines);
            Ok(())
        }

        fn ensure_walked(&self, deadline: Instant) {
            let need_walk = {
                let state = self.state.lock();
                match state.last_walk {
                    None => true,
                    Some(at) => at.elapsed() >= WALK_TTL,
                }
            };
            if !need_walk {
                return;
            }
            let mut seen: HashSet<PathBuf> = HashSet::new();
            let mut aborted = false;
            self.walk(|file| {
                if aborted {
                    return;
                }
                if Instant::now() >= deadline {
                    aborted = true;
                    return;
                }
                seen.insert(file.clone());
                let needs_read = {
                    let state = self.state.lock();
                    match state.ids.get(&file).and_then(|id| state.files.get(id)) {
                        Some(entry) => {
                            let fresh = std::fs::metadata(&file)
                                .ok()
                                .map(|m| {
                                    m.len() <= self.max_file_bytes
                                        && file_mtime_secs(&m) == entry.mtime_secs
                                })
                                .unwrap_or(false);
                            !fresh
                        }
                        None => true,
                    }
                };
                if needs_read {
                    let mut state = self.state.lock();
                    let _ = self.reindex_from_disk(&mut state, &file);
                }
            });
            let mut state = self.state.lock();
            if !aborted {
                let stale: Vec<u32> = state
                    .files
                    .keys()
                    .copied()
                    .filter(|id| {
                        state
                            .paths
                            .get(*id as usize)
                            .map(|p| !seen.contains(p))
                            .unwrap_or(true)
                    })
                    .collect();
                for id in stale {
                    Self::unindex(&mut state, id);
                }
                state.last_walk = Some(Instant::now());
            } else if state.last_walk.is_none() {
                state.last_walk = Some(
                    Instant::now()
                        .checked_sub(WALK_TTL.saturating_sub(Duration::from_secs(10)))
                        .unwrap_or_else(Instant::now),
                );
            }
        }

        fn candidate_ids(
            &self,
            terms: &[QueryTerm],
            min_terms: u32,
            focus_dirs: &HashSet<PathBuf>,
        ) -> Vec<(u32, u32)> {
            let state = self.state.lock();
            let mut matched: HashMap<u32, u32> = HashMap::new();
            for term in terms {
                let ids: HashSet<u32> = match term {
                    QueryTerm::Ascii(t) if t.is_ascii() && t.len() >= 3 => {
                        let mut acc: Option<HashSet<u32>> = None;
                        let mut missing = false;
                        for w in t.as_bytes().windows(3) {
                            let key = [w[0], w[1], w[2]];
                            match state.trigram_postings.get(&key) {
                                Some(set) => match acc.as_mut() {
                                    Some(cur) => {
                                        cur.retain(|id| set.contains(id));
                                        if cur.is_empty() {
                                            break;
                                        }
                                    }
                                    None => acc = Some(set.clone()),
                                },
                                None => {
                                    missing = true;
                                    break;
                                }
                            }
                        }
                        if missing {
                            HashSet::new()
                        } else {
                            acc.unwrap_or_default()
                        }
                    }
                    QueryTerm::Ascii(t) => {
                        let mut out: HashSet<u32> = HashSet::new();
                        for (token, posting) in state.postings.iter() {
                            if token.contains(t.as_str()) {
                                out.extend(posting.iter().copied());
                            }
                        }
                        out
                    }
                    QueryTerm::Cjk { grams, .. } => {
                        let mut iter = grams.iter();
                        let mut acc: Option<HashSet<u32>> = iter
                            .next()
                            .map(|g| state.postings.get(g).cloned().unwrap_or_default());
                        for g in iter {
                            let Some(cur) = acc.as_mut() else { break };
                            match state.postings.get(g) {
                                Some(posting) => cur.retain(|id| posting.contains(id)),
                                None => cur.clear(),
                            }
                            if cur.is_empty() {
                                break;
                            }
                        }
                        acc.unwrap_or_default()
                    }
                };
                for id in ids {
                    *matched.entry(id).or_insert(0) += 1;
                }
            }
            let is_focus_neighbor = |id: u32| -> bool {
                if focus_dirs.is_empty() {
                    return false;
                }
                state
                    .paths
                    .get(id as usize)
                    .and_then(|p| p.parent())
                    .map(|dir| focus_dirs.contains(dir))
                    .unwrap_or(false)
            };
            let mut out: Vec<(u32, u32, bool)> = matched
                .into_iter()
                .filter(|(_, count)| *count >= min_terms.max(1))
                .map(|(id, count)| (id, count, is_focus_neighbor(id)))
                .collect();
            out.sort_by(|a, b| {
                b.2.cmp(&a.2)
                    .then_with(|| b.1.cmp(&a.1))
                    .then_with(|| a.0.cmp(&b.0))
            });
            out.truncate(MAX_CANDIDATE_FILES);
            out.into_iter().map(|(id, count, _)| (id, count)).collect()
        }

        fn candidate_snapshot(&self, id: u32) -> Option<(PathBuf, u64, Arc<Vec<String>>)> {
            let state = self.state.lock();
            let path = state.paths.get(id as usize)?.clone();
            let entry = state.files.get(&id)?;
            Some((path, entry.mtime_secs, Arc::clone(&entry.lines)))
        }
    }

    impl Search {
        fn search_inner(
            &self,
            query: &str,
            limit: usize,
            focus: &[PathBuf],
        ) -> io::Result<Vec<SearchHit>> {
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
            let min_terms: u32 = if terms.len() >= 4 {
                (terms.len() as u32).div_ceil(2)
            } else if terms.is_empty() {
                0
            } else {
                1
            };

            let deadline = std::time::Instant::now()
                + std::time::Duration::from_secs(SEARCH_TIMEOUT_SECS);
            self.ensure_walked(deadline);

            if terms.is_empty() {
                return Ok(Vec::new());
            }

            let focus_dirs: HashSet<PathBuf> = focus
                .iter()
                .filter_map(|p| p.parent().map(Path::to_path_buf))
                .collect();
            let candidates = self.candidate_ids(&terms, min_terms, &focus_dirs);
            let keep = limit.saturating_mul(4).max(32);
            let mut scored: Vec<ScoredHit> = Vec::new();

            for (id, _term_count) in candidates {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let Some((path, indexed_mtime, mut lines)) = self.candidate_snapshot(id)
                else {
                    continue;
                };
                let disk_mtime = std::fs::metadata(&path).ok().map(|m| file_mtime_secs(&m));
                match disk_mtime {
                    Some(m) if m == indexed_mtime => {}
                    Some(_) => {
                        let mut state = self.state.lock();
                        let _ = self.reindex_from_disk(&mut state, &path);
                        match state.files.get(&id) {
                            Some(entry) => lines = Arc::clone(&entry.lines),
                            None => continue,
                        }
                    }
                    None => {
                        let mut state = self.state.lock();
                        Self::unindex(&mut state, id);
                        continue;
                    }
                }

                let mut file_hits = 0usize;
                for (idx, raw) in lines.iter().enumerate() {
                    let line_lower = raw.to_lowercase();
                    let mut score: u32 = 0;
                    for term in &terms {
                        if line_lower.contains(term.line_needle()) {
                            score += 1;
                        }
                    }
                    if !q_lower.is_empty() && line_lower.contains(&q_lower) {
                        score += 3;
                    }
                    if score < min_terms.max(1) {
                        continue;
                    }
                    scored.push(ScoredHit {
                        hit: SearchHit {
                            path: path.clone(),
                            line: idx as u32 + 1,
                            snippet: raw.trim().to_string(),
                            end_line: None,
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
            }

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

    impl IncrementalIndex for Search {
        fn reindex_file(&self, path: &Path) -> io::Result<()> {
            let mut state = self.state.lock();
            self.reindex_from_disk(&mut state, path)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            let mut state = self.state.lock();
            if let Some(id) = state.ids.get(path).copied() {
                Self::unindex(&mut state, id);
            }
            Ok(())
        }

        fn search(&self, query: &str, limit: usize) -> io::Result<Vec<SearchHit>> {
            self.search_inner(query, limit, &[])
        }

        fn search_with_focus(
            &self,
            query: &str,
            limit: usize,
            focus: &[PathBuf],
        ) -> io::Result<Vec<SearchHit>> {
            self.search_inner(query, limit, focus)
        }

        fn mark_walk_fresh(&self) {
            let mut state = self.state.lock();
            if state.last_walk.is_some() {
                state.last_walk = Some(Instant::now());
            }
        }
    }

}
