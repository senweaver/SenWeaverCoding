// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _};
use grep::matcher::Matcher;
use grep::regex::{RegexMatcher, RegexMatcherBuilder};
use grep::searcher::{
    BinaryDetection, Encoding, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch,
};
use ignore::overrides::OverrideBuilder;
use ignore::{WalkBuilder, WalkState};

const MAX_SEARCH_THREADS: usize = 12;

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub root: PathBuf,
    pub pattern: String,
    pub fixed_string: bool,
    pub case_sensitive: bool,
    pub smart_case: bool,
    pub whole_word: bool,
    pub multiline: bool,
    pub include_globs: Vec<String>,
    pub respect_ignore: bool,
    pub include_hidden: bool,
    pub max_file_size: Option<u64>,
    pub max_count_per_file: Option<u64>,
    pub context_before: usize,
    pub context_after: usize,
    pub encoding: Option<String>,
    pub timeout: Option<Duration>,
    pub max_total_matches: u64,
    pub collect_lines: bool,
}

impl SearchRequest {
    #[must_use]
    pub fn new(root: PathBuf, pattern: String) -> Self {
        Self {
            root,
            pattern,
            fixed_string: false,
            case_sensitive: true,
            smart_case: false,
            whole_word: false,
            multiline: false,
            include_globs: Vec::new(),
            respect_ignore: true,
            include_hidden: false,
            max_file_size: None,
            max_count_per_file: None,
            context_before: 0,
            context_after: 0,
            encoding: None,
            timeout: None,
            max_total_matches: u64::MAX,
            collect_lines: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LineMatch {
    pub line_number: u64,
    pub text: String,
    pub is_context: bool,
    pub submatches: Vec<(usize, usize)>,
}

#[derive(Debug)]
pub struct FileMatches {
    pub path: PathBuf,
    pub lines: Vec<LineMatch>,
    pub match_count: u64,
}

#[derive(Debug, Default)]
pub struct SearchOutcome {
    pub files: Vec<FileMatches>,
    pub total_matches: u64,
    pub truncated: bool,
    pub timed_out: bool,
}

pub fn search(req: &SearchRequest) -> anyhow::Result<SearchOutcome> {
    let pattern = if req.fixed_string {
        regex::escape(&req.pattern)
    } else {
        req.pattern.clone()
    };

    let mut matcher_builder = RegexMatcherBuilder::new();
    matcher_builder
        .case_smart(req.smart_case)
        .case_insensitive(!req.smart_case && !req.case_sensitive)
        .word(req.whole_word);
    if req.multiline {
        matcher_builder.multi_line(true).dot_matches_new_line(true);
    }
    let matcher = matcher_builder
        .build(&pattern)
        .map_err(|e| anyhow!("invalid search pattern: {e}"))?;

    let mut searcher_builder = SearcherBuilder::new();
    searcher_builder
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .multi_line(req.multiline)
        .before_context(req.context_before)
        .after_context(req.context_after);
    if let Some(label) = req.encoding.as_deref() {
        let encoding = Encoding::new(label)
            .map_err(|e| anyhow!("unsupported encoding '{label}': {e}"))?;
        searcher_builder.encoding(Some(encoding));
    }

    let mut walk_builder = WalkBuilder::new(&req.root);
    walk_builder
        .hidden(!req.include_hidden)
        .parents(req.respect_ignore)
        .ignore(req.respect_ignore)
        .git_ignore(req.respect_ignore)
        .git_global(req.respect_ignore)
        .git_exclude(req.respect_ignore)
        .follow_links(false)
        .max_filesize(req.max_file_size)
        .threads(
            std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(4)
                .min(MAX_SEARCH_THREADS),
        );
    if !req.include_globs.is_empty() {
        let mut override_builder = OverrideBuilder::new(&req.root);
        for glob in &req.include_globs {
            override_builder
                .add(glob)
                .with_context(|| format!("invalid glob '{glob}'"))?;
        }
        walk_builder.overrides(
            override_builder
                .build()
                .context("building glob override set")?,
        );
    }

    let deadline = req.timeout.map(|t| Instant::now() + t);
    let stop = AtomicBool::new(false);
    let timed_out = AtomicBool::new(false);
    let total = AtomicU64::new(0);
    let collected: Mutex<Vec<FileMatches>> = Mutex::new(Vec::new());

    walk_builder.build_parallel().run(|| {
        let matcher = matcher.clone();
        let mut searcher = searcher_builder.build();
        let stop = &stop;
        let timed_out = &timed_out;
        let total = &total;
        let collected = &collected;
        Box::new(move |entry| {
            if stop.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            if let Some(d) = deadline {
                if Instant::now() >= d {
                    timed_out.store(true, Ordering::Relaxed);
                    stop.store(true, Ordering::Relaxed);
                    return WalkState::Quit;
                }
            }
            let Ok(entry) = entry else {
                return WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                return WalkState::Continue;
            }
            let mut sink = CollectSink {
                matcher: &matcher,
                collect_lines: req.collect_lines,
                per_file_cap: req.max_count_per_file,
                multiline: req.multiline,
                lines: Vec::new(),
                match_count: 0,
            };
            if let Err(err) = searcher.search_path(&matcher, entry.path(), &mut sink) {
                tracing::debug!(
                    target: "tools.content_search",
                    path = %entry.path().display(),
                    error = %err,
                    "skipping unreadable file during search"
                );
                return WalkState::Continue;
            }
            if sink.match_count > 0 {
                let previous = total.fetch_add(sink.match_count, Ordering::Relaxed);
                if let Ok(mut guard) = collected.lock() {
                    guard.push(FileMatches {
                        path: entry.into_path(),
                        lines: sink.lines,
                        match_count: sink.match_count,
                    });
                }
                if previous.saturating_add(sink.match_count) >= req.max_total_matches {
                    stop.store(true, Ordering::Relaxed);
                    return WalkState::Quit;
                }
            }
            WalkState::Continue
        })
    });

    let mut files = collected.into_inner().unwrap_or_default();
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let timed_out = timed_out.load(Ordering::Relaxed);
    let total_matches = total.load(Ordering::Relaxed);
    Ok(SearchOutcome {
        files,
        total_matches,
        truncated: !timed_out
            && stop.load(Ordering::Relaxed)
            && total_matches >= req.max_total_matches,
        timed_out,
    })
}

struct CollectSink<'a> {
    matcher: &'a RegexMatcher,
    collect_lines: bool,
    per_file_cap: Option<u64>,
    multiline: bool,
    lines: Vec<LineMatch>,
    match_count: u64,
}

impl Sink for CollectSink<'_> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        self.match_count += 1;
        if self.collect_lines {
            let base_line = mat.line_number().unwrap_or(0);
            let text = String::from_utf8_lossy(mat.bytes());
            let trimmed = text.trim_end_matches('\n');
            for (i, raw_line) in trimmed.split('\n').enumerate() {
                let line = raw_line.trim_end_matches('\r');
                let submatches = if !self.multiline {
                    let mut subs: Vec<(usize, usize)> = Vec::new();
                    let _ = self.matcher.find_iter(line.as_bytes(), |m| {
                        subs.push((m.start(), m.end()));
                        true
                    });
                    subs
                } else {
                    Vec::new()
                };
                self.lines.push(LineMatch {
                    line_number: base_line + i as u64,
                    text: line.to_string(),
                    is_context: false,
                    submatches,
                });
            }
        }
        if let Some(cap) = self.per_file_cap {
            if self.match_count >= cap {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        if self.collect_lines {
            let text = String::from_utf8_lossy(ctx.bytes());
            let line = text.trim_end_matches('\n').trim_end_matches('\r');
            self.lines.push(LineMatch {
                line_number: ctx.line_number().unwrap_or(0),
                text: line.to_string(),
                is_context: true,
                submatches: Vec::new(),
            });
        }
        Ok(true)
    }
}
