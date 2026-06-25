// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::runtime::task_manager::TaskHandle;

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".senweavercoding",
    ".cursor",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];

#[derive(Clone)]
pub struct FileWatchConfig {
    pub root: PathBuf,
    pub poll_interval: Duration,
    pub debounce: Duration,
    pub extensions: Vec<String>,
    pub max_depth: usize,
    pub max_entries: usize,
}

impl FileWatchConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            poll_interval: Duration::from_secs(2),
            debounce: Duration::from_secs(3),
            extensions: Vec::new(),
            max_depth: 12,
            max_entries: 20_000,
        }
    }

    pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
        self.extensions = extensions
            .into_iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        self
    }

    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    fn matches_extension(&self, path: &Path) -> bool {
        if self.extensions.is_empty() {
            return true;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => self.extensions.iter().any(|e| e == &ext.to_ascii_lowercase()),
            None => false,
        }
    }
}

pub type ChangeCallback =
    Arc<dyn Fn(Vec<PathBuf>) + Send + Sync + 'static>;

pub fn spawn_file_watch(config: FileWatchConfig, on_change: ChangeCallback) -> TaskHandle {
    let name = format!("agent.observe.file_watch.{}", config.root.display());
    crate::runtime::task_manager::spawn_supervised(name, async move {
        run_watch_loop(config, on_change).await;
    })
}

async fn run_watch_loop(config: FileWatchConfig, on_change: ChangeCallback) {
    tracing::info!(
        target: "agent.observe.file_watch",
        root = %config.root.display(),
        debounce_ms = config.debounce.as_millis() as u64,
        "file-watch trigger started",
    );

    let mut snapshot: HashMap<PathBuf, SystemTime> = scan(&config);
    let mut pending: HashMap<PathBuf, ()> = HashMap::new();
    let mut last_change_at: Option<Instant> = None;

    loop {
        if crate::security::estop::is_kill_all() {
            tracing::warn!(
                target: "agent.observe.file_watch",
                "estop kill_all engaged; file-watch trigger exiting",
            );
            return;
        }

        tokio::time::sleep(config.poll_interval).await;

        let current = scan(&config);
        for (path, mtime) in &current {
            let changed = match snapshot.get(path) {
                Some(prev) => mtime > prev,
                None => true,
            };
            if changed {
                pending.insert(path.clone(), ());
            }
        }
        snapshot = current;

        if !pending.is_empty() {
            last_change_at = Some(Instant::now());
        }

        if let Some(changed_at) = last_change_at {
            if !pending.is_empty() && changed_at.elapsed() >= config.debounce {
                let mut paths: Vec<PathBuf> = pending.keys().cloned().collect();
                paths.sort();
                pending.clear();
                last_change_at = None;
                tracing::info!(
                    target: "agent.observe.file_watch",
                    changed = paths.len(),
                    "file-watch debounce elapsed; firing change callback",
                );
                on_change(paths);
            }
        }
    }
}

fn scan(config: &FileWatchConfig) -> HashMap<PathBuf, SystemTime> {
    let mut out = HashMap::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(config.root.clone(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > config.max_depth || out.len() >= config.max_entries {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if IGNORED_DIRS.contains(&name) || name.starts_with('.') {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if file_type.is_file() && config.matches_extension(&path) {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        out.insert(path, modified);
                    }
                }
            }
            if out.len() >= config.max_entries {
                break;
            }
        }
    }

    out
}
