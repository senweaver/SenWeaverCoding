// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use crossterm::event::{self, KeyCode};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Clone)]
enum GitignorePattern {

    Name(String),

    Extension(String),

    Dir(String),

    Prefix(String),
}

impl GitignorePattern {

    fn matches_name(&self, name: &str, is_dir: bool) -> bool {
        match self {
            GitignorePattern::Name(n) => unicase_eq(name, n),
            GitignorePattern::Extension(ext) => name
                .rsplit_once('.')
                .map(|(_, e)| unicase_eq(e, ext))
                .unwrap_or(false),
            GitignorePattern::Dir(n) => is_dir && unicase_eq(name, n),
            GitignorePattern::Prefix(p) => name.to_lowercase().starts_with(&p.to_lowercase()),
        }
    }
}

fn unicase_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn parse_gitignore_lines(content: &str) -> Vec<GitignorePattern> {
    content
        .lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                return None;
            }

            let line = line.strip_prefix('/').unwrap_or(line);
            if line.is_empty() {
                return None;
            }

            if let Some(dir) = line.strip_suffix('/') {
                if !dir.contains('/') && !dir.contains('*') {
                    return Some(GitignorePattern::Dir(dir.to_string()));
                }
                return None;
            }

            if let Some(rest) = line.strip_prefix("*.") {
                if !rest.contains('/') && !rest.contains('*') {
                    return Some(GitignorePattern::Extension(rest.to_string()));
                }
                return None;
            }

            if let Some(prefix) = line.strip_suffix('*') {
                if !prefix.contains('/') {
                    return Some(GitignorePattern::Prefix(prefix.to_string()));
                }
                return None;
            }

            if line.contains('/') || line.contains('*') {
                return None;
            }
            Some(GitignorePattern::Name(line.to_string()))
        })
        .collect()
}

fn load_gitignore(workspace_root: &Path) -> Vec<GitignorePattern> {
    let path = workspace_root.join(".gitignore");
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    parse_gitignore_lines(&content)
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
}

impl Entry {
    pub fn display_name(&self, root: &Path) -> String {
        self.path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| self.path.display().to_string())
    }
}

enum ScanRequest {
    Scan {
        generation: u64,
        dir: PathBuf,
        gitignore: Vec<GitignorePattern>,
    },
    Preview {
        generation: u64,
        path: PathBuf,
    },
}

enum ScanResult {
    Scan {
        generation: u64,
        dir: PathBuf,
        entries: Vec<Entry>,
        mtime: Option<std::time::SystemTime>,
    },
    Preview {
        generation: u64,
        text: String,
    },
}

#[derive(Debug, Default)]
pub struct FileViewerState {

    pub current_dir: Option<PathBuf>,

    pub entries: Vec<Entry>,

    pub selected: usize,

    pub preview: Option<String>,

    pub status: Option<String>,
    list_state: ListState,

    gitignore_patterns: Vec<GitignorePattern>,

    gitignore_loaded: bool,

    pub search_mode: bool,

    pub search_query: String,

    filtered_entries: Vec<Entry>,

    req_tx: Option<std::sync::mpsc::Sender<ScanRequest>>,
    res_rx: Option<std::sync::mpsc::Receiver<ScanResult>>,
    scan_generation: u64,
    preview_generation: u64,
    requested_dir: Option<PathBuf>,
    loading: bool,
    current_dir_mtime: Option<std::time::SystemTime>,
    last_mtime_check: Option<std::time::Instant>,
}

impl FileViewerState {
    pub fn new() -> Self {
        let mut s = Self::default();
        s.list_state.select(Some(0));
        s
    }

    fn ensure_worker(&mut self) {
        if self.req_tx.is_some() {
            return;
        }
        let (req_tx, req_rx) = std::sync::mpsc::channel::<ScanRequest>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<ScanResult>();
        let _ = std::thread::Builder::new()
            .name("tui-file-viewer".into())
            .spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    match req {
                        ScanRequest::Scan {
                            generation,
                            dir,
                            gitignore,
                        } => {
                            let entries = scan_dir(&dir, &gitignore);
                            let mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
                            if res_tx
                                .send(ScanResult::Scan {
                                    generation,
                                    dir,
                                    entries,
                                    mtime,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        ScanRequest::Preview { generation, path } => {
                            let text = read_preview(&path);
                            if res_tx
                                .send(ScanResult::Preview { generation, text })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            });
        self.req_tx = Some(req_tx);
        self.res_rx = Some(res_rx);
    }

    fn send_scan(&mut self, dir: PathBuf) {
        self.ensure_worker();
        self.scan_generation += 1;
        self.requested_dir = Some(dir.clone());
        self.loading = true;
        if let Some(tx) = &self.req_tx {
            let _ = tx.send(ScanRequest::Scan {
                generation: self.scan_generation,
                dir,
                gitignore: self.gitignore_patterns.clone(),
            });
        }
    }

    fn send_preview(&mut self) {
        let entry = self.display_entries().get(self.selected).cloned();
        let Some(entry) = entry else {
            self.preview = None;
            return;
        };
        if entry.is_dir {
            self.preview = None;
            return;
        }
        self.ensure_worker();
        self.preview_generation += 1;
        if let Some(tx) = &self.req_tx {
            let _ = tx.send(ScanRequest::Preview {
                generation: self.preview_generation,
                path: entry.path,
            });
        }
    }

    pub fn poll(&mut self) {
        let mut latest_scan: Option<(PathBuf, Vec<Entry>, Option<std::time::SystemTime>)> = None;
        let mut latest_preview: Option<String> = None;
        if let Some(rx) = &self.res_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    ScanResult::Scan {
                        generation,
                        dir,
                        entries,
                        mtime,
                    } => {
                        if generation == self.scan_generation {
                            latest_scan = Some((dir, entries, mtime));
                        }
                    }
                    ScanResult::Preview { generation, text } => {
                        if generation == self.preview_generation {
                            latest_preview = Some(text);
                        }
                    }
                }
            }
        }

        if let Some((dir, entries, mtime)) = latest_scan {
            self.current_dir = Some(dir);
            self.entries = entries;
            self.current_dir_mtime = mtime;
            self.loading = false;
            if self.selected >= self.entries.len() {
                self.selected = self.entries.len().saturating_sub(1);
            }
            self.list_state.select(Some(self.selected));
            if self.search_mode && !self.search_query.is_empty() {
                self.rebuild_filter();
            }
            self.send_preview();
        }

        if let Some(text) = latest_preview {
            self.preview = Some(text);
        }
    }

    fn ensure_loaded(&mut self, root: &Path) {
        if !self.gitignore_loaded {
            self.gitignore_patterns = load_gitignore(root);
            self.gitignore_loaded = true;
        }
        let target = self.current_dir.clone().unwrap_or_else(|| root.to_path_buf());
        if self.requested_dir.as_deref() != Some(target.as_path()) {
            self.send_scan(target);
            return;
        }

        let now = std::time::Instant::now();
        let due = self
            .last_mtime_check
            .map_or(true, |t| now.duration_since(t) >= std::time::Duration::from_secs(1));
        if due && !self.loading {
            self.last_mtime_check = Some(now);
            let current_mtime = std::fs::metadata(&target).and_then(|m| m.modified()).ok();
            if current_mtime != self.current_dir_mtime {
                self.send_scan(target);
            }
        }
    }

    pub fn display_entries(&self) -> &[Entry] {
        if self.search_mode && !self.search_query.is_empty() {
            &self.filtered_entries
        } else {
            &self.entries
        }
    }

    fn rebuild_filter(&mut self) {
        let q = self.search_query.to_lowercase();
        self.filtered_entries = self
            .entries
            .iter()
            .filter(|e| {
                let name = e
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                name.contains(&q)
            })
            .cloned()
            .collect();

        let max = self.filtered_entries.len().saturating_sub(1);
        self.selected = self.selected.min(max);
        self.list_state.select(Some(self.selected));
    }

    pub fn tick(&mut self, root: &Path) {
        self.poll();
        self.ensure_loaded(root);
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.display_entries().len();
        if len == 0 {
            return;
        }
        let next = (self.selected as isize + delta).clamp(0, len as isize - 1);
        self.selected = next as usize;
        self.list_state.select(Some(self.selected));
        self.send_preview();
    }
}

fn should_always_skip(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".sen"
            | ".cargo"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | ".cache"
            | "out"
    )
}

fn scan_dir(dir: &Path, gitignore: &[GitignorePattern]) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = read
        .filter_map(|r| r.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if should_always_skip(&name) {
                return None;
            }
            let file_type = entry.file_type().ok()?;
            let is_dir = file_type.is_dir();

            if gitignore.iter().any(|p| p.matches_name(&name, is_dir)) {
                return None;
            }
            Some(Entry {
                path: entry.path(),
                is_dir,
            })
        })
        .collect();
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a
            .path
            .file_name()
            .unwrap_or_default()
            .cmp(b.path.file_name().unwrap_or_default()),
    });
    out
}

fn read_preview(path: &Path) -> String {
    const MAX: u64 = 4 * 1024;
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX => {
            let bytes = std::fs::read(path).unwrap_or_default();
            let slice = &bytes[..(MAX as usize).min(bytes.len())];
            String::from_utf8_lossy(slice).into_owned()
                + &format!("\n\n… truncated at {MAX} bytes (file is {} B)", meta.len())
        }
        Ok(_) => std::fs::read_to_string(path).unwrap_or_else(|e| format!("(read error: {e})")),
        Err(e) => format!("(metadata error: {e})"),
    }
}

pub fn draw(
    f: &mut Frame,
    state: &mut FileViewerState,
    workspace: &Path,
    open_files: &[(PathBuf, Option<chrono::DateTime<chrono::Utc>>)],
    area: Rect,
) {
    state.tick(workspace);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(area);

    draw_browse(f, state, workspace, cols[0]);
    draw_preview(f, state, cols[1]);
    draw_open_list(f, open_files, workspace, cols[2]);
}

fn draw_browse(f: &mut Frame, state: &mut FileViewerState, root: &Path, area: Rect) {

    let (list_area, search_area) = if state.search_mode {
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let rel_title = state
        .current_dir
        .as_deref()
        .and_then(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = if rel_title.is_empty() {
        "Workspace".to_string()
    } else {
        format!("Workspace / {rel_title}")
    };

    let query_lower = state.search_query.to_lowercase();
    let show_filtered = state.search_mode && !state.search_query.is_empty();
    let display = state.display_entries();

    let items: Vec<ListItem> = display
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let icon = if e.is_dir { "▸" } else { "·" };
            let name = e
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| e.path.display().to_string());
            let is_selected = i == state.selected;
            let base_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if e.is_dir {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            if show_filtered && !query_lower.is_empty() {
                let name_lower = name.to_lowercase();
                if let Some(pos) = name_lower.find(&query_lower) {
                    let before = name[..pos].to_string();
                    let matched = name[pos..pos + query_lower.len()].to_string();
                    let after = name[pos + query_lower.len()..].to_string();
                    let hi_style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::UNDERLINED)
                    };
                    let line = Line::from(vec![
                        Span::styled(format!("{icon} {before}"), base_style),
                        Span::styled(matched, hi_style),
                        Span::styled(after, base_style),
                    ]);
                    return ListItem::new(line);
                }
            }
            ListItem::new(format!("{icon} {name}")).style(base_style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_stateful_widget(list, list_area, &mut state.list_state);

    if let Some(sa) = search_area {
        let prompt = format!("/{}", state.search_query);
        let para = Paragraph::new(prompt).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Search (Esc=cancel, Enter=confirm)")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(para, sa);
    }
}

fn draw_preview(f: &mut Frame, state: &FileViewerState, area: Rect) {
    let body = state
        .preview
        .clone()
        .unwrap_or_else(|| "(select a file to preview)".to_string());
    let para = Paragraph::new(body)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Preview (4 KB max)"),
        );
    f.render_widget(para, area);
}

fn draw_open_list(
    f: &mut Frame,
    open_files: &[(PathBuf, Option<chrono::DateTime<chrono::Utc>>)],
    root: &Path,
    area: Rect,
) {
    let items: Vec<ListItem> = if open_files.is_empty() {
        vec![ListItem::new(Span::styled(
            "No files marked open yet.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        open_files
            .iter()
            .map(|(p, ts)| {
                let rel = p
                    .strip_prefix(root)
                    .map(|r| r.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| p.display().to_string());
                let ts_str = ts
                    .as_ref()
                    .map(|t| t.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| "--:--".to_string());
                ListItem::new(Line::from(vec![
                    Span::styled(ts_str, Style::default().fg(Color::DarkGray)),
                    Span::raw("  "),
                    Span::raw(rel),
                ]))
            })
            .collect()
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Open files (session)"),
    );
    f.render_widget(list, area);
}

#[derive(Debug, Clone)]
pub enum FileViewerAction {
    Noop,
    Open { path: PathBuf },
    Toast(String),
}

pub fn handle_key(
    state: &mut FileViewerState,
    workspace: &Path,
    key: event::KeyEvent,
) -> FileViewerAction {
    state.tick(workspace);

    if state.search_mode {
        match key.code {
            KeyCode::Esc => {
                state.search_mode = false;
                state.search_query.clear();
                state.filtered_entries.clear();

                let max = state.entries.len().saturating_sub(1);
                state.selected = state.selected.min(max);
                state.list_state.select(Some(state.selected));
                state.send_preview();
            }
            KeyCode::Enter => {

                state.search_mode = false;
                state.send_preview();
            }
            KeyCode::Backspace => {
                state.search_query.pop();
                state.rebuild_filter();
                state.send_preview();
            }
            KeyCode::Char(c) => {
                state.search_query.push(c);
                state.rebuild_filter();
                state.send_preview();
            }

            KeyCode::Down => {
                state.move_selection(1);
            }
            KeyCode::Up => {
                state.move_selection(-1);
            }
            _ => {}
        }
        return FileViewerAction::Noop;
    }

    match key.code {

        KeyCode::Char('/') => {
            state.search_mode = true;
            state.search_query.clear();
            state.filtered_entries.clear();
            FileViewerAction::Noop
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.move_selection(1);
            FileViewerAction::Noop
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_selection(-1);
            FileViewerAction::Noop
        }
        KeyCode::Enter => {
            let Some(entry) = state.display_entries().get(state.selected).cloned() else {
                return FileViewerAction::Noop;
            };
            if entry.is_dir {
                state.current_dir = Some(entry.path.clone());
                state.entries.clear();
                state.filtered_entries.clear();
                state.preview = None;
                state.selected = 0;
                state.send_scan(entry.path);
                FileViewerAction::Noop
            } else {
                FileViewerAction::Open { path: entry.path }
            }
        }
        KeyCode::Backspace | KeyCode::Left => {
            if let Some(parent) = state
                .current_dir
                .as_deref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
            {
                if parent.starts_with(workspace) || parent == *workspace {
                    state.current_dir = Some(parent.clone());
                    state.entries.clear();
                    state.filtered_entries.clear();
                    state.preview = None;
                    state.selected = 0;
                    state.send_scan(parent);
                }
            }
            FileViewerAction::Noop
        }
        _ => FileViewerAction::Noop,
    }
}
