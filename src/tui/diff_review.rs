// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use crossterm::event::{self, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use super::edit_batch_registry::{
    EditBatchRegistry, HunkStatus, PendingEdit, PendingStatus,
};
use crate::observability::tui_metrics;

#[derive(Debug, Default)]
pub struct DiffReviewState {

    pub selected_entry: usize,

    pub selected_hunk: usize,

    pub drill_into_hunk: bool,

    pub scroll: u16,

    pub toast: Option<String>,

    pub reverting_entries: std::collections::HashSet<u64>,

    preview_cache: std::collections::HashMap<u64, (u64, std::sync::Arc<Vec<HunkView>>)>,
    list_state: ListState,
}

impl DiffReviewState {
    pub fn new() -> Self {
        let mut s = Self::default();
        s.list_state.select(Some(0));
        s
    }

    pub fn cached_hunks(
        &mut self,
        entry_id: u64,
        diff: Option<&str>,
    ) -> std::sync::Arc<Vec<HunkView>> {
        let Some(diff) = diff else {
            return std::sync::Arc::new(Vec::new());
        };
        let fingerprint = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            diff.hash(&mut h);
            h.finish()
        };
        if let Some((cached_fp, hunks)) = self.preview_cache.get(&entry_id) {
            if *cached_fp == fingerprint {
                return hunks.clone();
            }
        }
        let parsed = std::sync::Arc::new(parse_unified_diff(diff));
        self.preview_cache
            .insert(entry_id, (fingerprint, parsed.clone()));
        parsed
    }

    pub fn clamp_selection(&mut self, entry_count: usize, hunk_count: usize) {
        if entry_count == 0 {
            self.selected_entry = 0;
            self.selected_hunk = 0;
            self.list_state.select(None);
            return;
        }
        if self.selected_entry >= entry_count {
            self.selected_entry = entry_count - 1;
        }
        self.list_state.select(Some(self.selected_entry));
        if hunk_count == 0 {
            self.selected_hunk = 0;
        } else if self.selected_hunk >= hunk_count {
            self.selected_hunk = hunk_count - 1;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLine {
    Context(String),
    Added(String),
    Removed(String),
}

#[derive(Debug, Clone)]
pub struct HunkView {
    pub header: String,
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub lines: Vec<HunkLine>,
}

pub fn parse_unified_diff(diff: &str) -> Vec<HunkView> {
    let mut hunks = Vec::new();
    let mut current: Option<HunkView> = None;
    for line in diff.lines() {
        if let Some(stripped) = line.strip_prefix("@@") {
            if let Some(h) = current.take() {
                hunks.push(h);
            }
            if let Some((old_start, old_len, new_start, new_len)) = parse_hunk_header(stripped) {
                current = Some(HunkView {
                    header: line.to_string(),
                    old_start,
                    old_len,
                    new_start,
                    new_len,
                    lines: Vec::new(),
                });
            }
            continue;
        }
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            if let Some(h) = current.take() {
                hunks.push(h);
            }
            continue;
        }
        let Some(ref mut hunk) = current else {
            continue;
        };
        if let Some(added) = line.strip_prefix('+') {
            hunk.lines.push(HunkLine::Added(added.to_string()));
        } else if let Some(removed) = line.strip_prefix('-') {
            hunk.lines.push(HunkLine::Removed(removed.to_string()));
        } else if let Some(ctx) = line.strip_prefix(' ') {
            hunk.lines.push(HunkLine::Context(ctx.to_string()));
        } else if line.is_empty() {
            hunk.lines.push(HunkLine::Context(String::new()));
        }
    }
    if let Some(h) = current {
        hunks.push(h);
    }
    hunks
}

fn parse_hunk_header(after_at_at: &str) -> Option<(u32, u32, u32, u32)> {
    let trimmed = after_at_at.trim_end_matches(|c: char| c != '@' && !c.is_ascii_whitespace());
    let inside = trimmed
        .trim()
        .trim_end_matches('@')
        .trim();
    let mut parts = inside.split_whitespace();
    let old = parts.next()?;
    let new = parts.next()?;
    let (old_start, old_len) = parse_range(old.strip_prefix('-')?)?;
    let (new_start, new_len) = parse_range(new.strip_prefix('+')?)?;
    Some((old_start, old_len, new_start, new_len))
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    let mut iter = s.splitn(2, ',');
    let start: u32 = iter.next()?.parse().ok()?;
    let len: u32 = iter.next().and_then(|v| v.parse().ok()).unwrap_or(1);
    Some((start, len))
}

pub fn invert_hunk(hunk: &HunkView) -> HunkView {
    let lines: Vec<HunkLine> = hunk
        .lines
        .iter()
        .map(|l| match l {
            HunkLine::Added(s) => HunkLine::Removed(s.clone()),
            HunkLine::Removed(s) => HunkLine::Added(s.clone()),
            HunkLine::Context(s) => HunkLine::Context(s.clone()),
        })
        .collect();
    let header = format!(
        "@@ -{},{} +{},{} @@",
        hunk.new_start, hunk.new_len, hunk.old_start, hunk.old_len
    );
    HunkView {
        header,
        old_start: hunk.new_start,
        old_len: hunk.new_len,
        new_start: hunk.old_start,
        new_len: hunk.old_len,
        lines,
    }
}

pub fn hunks_to_unified_diff(path: &Path, hunks: &[HunkView]) -> String {
    let mut out = String::new();
    let path_str = path.display();
    out.push_str(&format!("--- a/{path_str}\n"));
    out.push_str(&format!("+++ b/{path_str}\n"));
    for h in hunks {
        out.push_str(&h.header);
        if !h.header.ends_with('\n') {
            out.push('\n');
        }
        for line in &h.lines {
            match line {
                HunkLine::Context(s) => {
                    out.push(' ');
                    out.push_str(s);
                    out.push('\n');
                }
                HunkLine::Added(s) => {
                    out.push('+');
                    out.push_str(s);
                    out.push('\n');
                }
                HunkLine::Removed(s) => {
                    out.push('-');
                    out.push_str(s);
                    out.push('\n');
                }
            }
        }
    }
    out
}

pub fn revert_file_entry(entry: &PendingEdit, workspace: &Path) -> anyhow::Result<String> {
    let history = crate::tools::edit_history::EditHistory::new(workspace.to_path_buf());
    if let Some(batch_id) = entry.edit_batch_id.as_deref() {
        let reverted = history.revert_batch(batch_id)?;
        if reverted.is_empty() {

            history.revert_to_latest(Path::new(&entry.path))?;
            Ok(format!(
                "reverted {} via revert_to_latest (batch journal missing)",
                entry.path
            ))
        } else {
            Ok(format!(
                "reverted {} file(s) via batch {}",
                reverted.len(),
                batch_id
            ))
        }
    } else {
        history.revert_to_latest(Path::new(&entry.path))?;
        Ok(format!(
            "reverted {} via revert_to_latest (no batch id)",
            entry.path
        ))
    }
}

pub async fn revert_single_hunk(
    entry: &PendingEdit,
    hunk_index: usize,
    workspace: &Path,
) -> anyhow::Result<String> {
    let diff = entry
        .diff
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("pending edit has no diff text  -  hunk revert impossible"))?;
    let hunks = parse_unified_diff(diff);
    let target = hunks.get(hunk_index).ok_or_else(|| {
        anyhow::anyhow!(
            "hunk index {hunk_index} out of range (parsed {} hunks)",
            hunks.len()
        )
    })?;
    let inverted = invert_hunk(target);
    let patch = hunks_to_unified_diff(Path::new(&entry.path), &[inverted]);

    let path_buf = resolve_path(workspace, Path::new(&entry.path));
    let op = crate::apply_model::edit_op::EditOp::ApplyHunk {
        path: path_buf,
        diff: patch,
        fuzz: 2,
        scope_anchor: None,
    };
    let batch = crate::apply_model::edit_op::EditBatch {
        batch_id: uuid::Uuid::new_v4().to_string(),
        correlation_id: entry.edit_batch_id.clone(),
        origin: crate::apply_model::edit_op::EditOrigin::DiffSession,
        ops: vec![op],
        atomic: true,
    };

    let history = crate::tools::edit_history::EditHistory::new(workspace.to_path_buf());
    let applier = crate::apply_model::ops_applier::OpsApplier::default_for_workspace(workspace)
        .with_edit_history(history);
    let outcome = applier.apply_batch(batch).await?;
    Ok(outcome.batch_id)
}

fn resolve_path(workspace: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    }
}

pub fn draw(
    f: &mut Frame,
    state: &mut DiffReviewState,
    registry: &EditBatchRegistry,
    area: Rect,
) {
    let entries: Vec<&PendingEdit> = registry.entries_newest_first().collect();
    let hunk_count = entries
        .get(state.selected_entry)
        .map(|e| e.hunk_status.len())
        .unwrap_or(0);
    state.clamp_selection(entries.len(), hunk_count);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(52),
            Constraint::Percentage(20),
        ])
        .split(area);

    draw_entry_list(f, state, &entries, cols[0]);
    let selected_entry = entries.get(state.selected_entry).copied();
    draw_preview(f, state, selected_entry, cols[1]);
    draw_legend(f, state, cols[2]);
}

fn draw_entry_list(
    f: &mut Frame,
    state: &mut DiffReviewState,
    entries: &[&PendingEdit],
    area: Rect,
) {
    let reverting = state.reverting_entries.clone();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let marker = if reverting.contains(&e.id) {
                super::theme::spinner_frame_now()
            } else {
                match e.status() {
                    PendingStatus::Pending => "•",
                    PendingStatus::Applied => "A",
                    PendingStatus::Reverted => "R",
                    PendingStatus::PartiallyReverted => "~",
                }
            };
            let batch_hint = e
                .edit_batch_id
                .as_deref()
                .map(|id| format!(" [{}]", id.get(..8.min(id.len())).unwrap_or(id)))
                .unwrap_or_default();
            let line = format!(
                "{marker} {} (+{}/-{}){batch_hint}",
                e.path, e.additions, e.deletions
            );
            let style = if i == state.selected_entry {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                match e.status() {
                    PendingStatus::Applied => Style::default().fg(Color::Green),
                    PendingStatus::Reverted => Style::default().fg(Color::Red),
                    PendingStatus::PartiallyReverted => Style::default().fg(Color::Yellow),
                    PendingStatus::Pending => Style::default(),
                }
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let title = format!("Diff Queue ({})", entries.len());
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_stateful_widget(list, area, &mut state.list_state);
}

fn draw_preview(
    f: &mut Frame,
    state: &mut DiffReviewState,
    entry: Option<&PendingEdit>,
    area: Rect,
) {
    let Some(entry) = entry else {
        let p = Paragraph::new("No pending edits.  Agent hasn't touched any files yet.")
            .block(Block::default().borders(Borders::ALL).title("Preview"));
        f.render_widget(p, area);
        return;
    };

    let hunks = state.cached_hunks(entry.id, entry.diff.as_deref());
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            entry.path.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("+{}/-{}", entry.additions, entry.deletions),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(
            entry
                .edit_batch_id
                .clone()
                .unwrap_or_else(|| "(no batch id)".into()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(if entry.from_inline_edit {
            "  [inline]"
        } else {
            ""
        }),
    ]));
    lines.push(Line::from(""));

    if hunks.is_empty() {
        if let Some(text) = entry.diff.as_deref() {
            for raw in text.lines() {
                lines.push(Line::from(raw.to_string()));
            }
        } else {
            lines.push(Line::from("(no diff text available)"));
        }
    } else {
        for (hi, hunk) in hunks.iter().enumerate() {
            let hunk_focus = hi == state.selected_hunk;
            if state.drill_into_hunk && !hunk_focus {
                continue;
            }
            let status = entry
                .hunk_status
                .get(hi)
                .copied()
                .unwrap_or(HunkStatus::Pending);
            let marker = match status {
                HunkStatus::Applied => "[A]",
                HunkStatus::Reverted => "[R]",
                HunkStatus::Pending => "[ ]",
            };
            let header_style = if hunk_focus {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(Color::DarkGray)),
                Span::styled(hunk.header.clone(), header_style),
            ]));
            for hl in &hunk.lines {
                let (prefix, span_style, body) = match hl {
                    HunkLine::Added(s) => ("+", Style::default().fg(Color::Green), s.clone()),
                    HunkLine::Removed(s) => ("-", Style::default().fg(Color::Red), s.clone()),
                    HunkLine::Context(s) => (" ", Style::default(), s.clone()),
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), span_style),
                    Span::styled(body, span_style),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    let title = if state.drill_into_hunk {
        format!(
            "Hunk {}/{}",
            state.selected_hunk + 1,
            entry.hunk_status.len().max(1)
        )
    } else {
        "Preview".to_string()
    };
    let para = Paragraph::new(lines)
        .scroll((state.scroll, 0))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(para, area);
}

fn draw_legend(f: &mut Frame, state: &DiffReviewState, area: Rect) {
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            "Diff Review",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("j / k        next / prev entry"),
        Line::from("n / p        next / prev hunk"),
        Line::from("Enter        drill into hunk"),
        Line::from("Esc          exit hunk drill"),
        Line::from("A            apply whole file"),
        Line::from("R            reject whole file"),
        Line::from("a            apply hunk"),
        Line::from("r            reject hunk"),
        Line::from("c            comment on entry"),
        Line::from("PgUp/PgDn    scroll preview"),
    ];
    if let Some(toast) = state.toast.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            toast.to_string(),
            Style::default().fg(Color::Green),
        )));
    }
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Keys"));
    f.render_widget(para, area);
}

#[derive(Debug, Clone)]
pub enum DiffReviewAction {
    Noop,
    RevertFile { entry_id: u64 },
    RevertHunk { entry_id: u64, hunk_index: usize },
    MarkApplied { entry_id: u64, hunk_index: Option<usize> },
    Toast(String),
}

pub fn handle_key(
    state: &mut DiffReviewState,
    registry: &EditBatchRegistry,
    key: event::KeyEvent,
) -> DiffReviewAction {
    let entries: Vec<&PendingEdit> = registry.entries_newest_first().collect();
    let hunk_count = entries
        .get(state.selected_entry)
        .map(|e| e.hunk_status.len())
        .unwrap_or(0);
    state.clamp_selection(entries.len(), hunk_count);
    let entry = entries.get(state.selected_entry).copied();
    let entry_id = entry.map(|e| e.id);

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if !entries.is_empty() {
                state.selected_entry = (state.selected_entry + 1).min(entries.len() - 1);
                state.selected_hunk = 0;
                state.scroll = 0;
                state.list_state.select(Some(state.selected_entry));
            }
            DiffReviewAction::Noop
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if state.selected_entry > 0 {
                state.selected_entry -= 1;
                state.selected_hunk = 0;
                state.scroll = 0;
                state.list_state.select(Some(state.selected_entry));
            }
            DiffReviewAction::Noop
        }
        KeyCode::Char('n') => {
            if hunk_count > 0 {
                state.selected_hunk = (state.selected_hunk + 1).min(hunk_count - 1);
            }
            DiffReviewAction::Noop
        }
        KeyCode::Char('p') => {
            if state.selected_hunk > 0 {
                state.selected_hunk -= 1;
            }
            DiffReviewAction::Noop
        }
        KeyCode::Enter => {
            state.drill_into_hunk = true;
            DiffReviewAction::Noop
        }
        KeyCode::Esc => {
            state.drill_into_hunk = false;
            DiffReviewAction::Noop
        }
        KeyCode::PageDown => {
            state.scroll = state.scroll.saturating_add(10);
            DiffReviewAction::Noop
        }
        KeyCode::PageUp => {
            state.scroll = state.scroll.saturating_sub(10);
            DiffReviewAction::Noop
        }
        KeyCode::Char('A') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(id) = entry_id {
                tui_metrics::incr_tui_diff_review_apply_file();
                DiffReviewAction::MarkApplied {
                    entry_id: id,
                    hunk_index: None,
                }
            } else {
                DiffReviewAction::Noop
            }
        }
        KeyCode::Char('R') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(id) = entry_id {
                tui_metrics::incr_tui_diff_review_reject_file();
                DiffReviewAction::RevertFile { entry_id: id }
            } else {
                DiffReviewAction::Noop
            }
        }
        KeyCode::Char('a') => {
            if let Some(id) = entry_id {
                tui_metrics::incr_tui_diff_review_apply_hunk();
                DiffReviewAction::MarkApplied {
                    entry_id: id,
                    hunk_index: Some(state.selected_hunk),
                }
            } else {
                DiffReviewAction::Noop
            }
        }
        KeyCode::Char('r') => {
            if let Some(id) = entry_id {
                tui_metrics::incr_tui_diff_review_reject_hunk();
                DiffReviewAction::RevertHunk {
                    entry_id: id,
                    hunk_index: state.selected_hunk,
                }
            } else {
                DiffReviewAction::Noop
            }
        }
        KeyCode::Char('c') => {
            tui_metrics::incr_tui_diff_review_comment();
            DiffReviewAction::Toast(
                "Comments attach to the current entry (routed via session event bus).".into(),
            )
        }
        _ => DiffReviewAction::Noop,
    }
}

pub fn mark_applied(
    registry: &mut EditBatchRegistry,
    entry_id: u64,
    hunk_index: Option<usize>,
) {
    if let Some(entry) = registry.get_mut_by_id(entry_id) {
        match hunk_index {
            Some(i) => {
                if let Some(slot) = entry.hunk_status.get_mut(i) {
                    *slot = HunkStatus::Applied;
                }
            }
            None => {
                for s in entry.hunk_status.iter_mut() {
                    *s = HunkStatus::Applied;
                }
            }
        }
    }
}

pub fn mark_reverted(
    registry: &mut EditBatchRegistry,
    entry_id: u64,
    hunk_index: Option<usize>,
) {
    if let Some(entry) = registry.get_mut_by_id(entry_id) {
        match hunk_index {
            Some(i) => {
                if let Some(slot) = entry.hunk_status.get_mut(i) {
                    *slot = HunkStatus::Reverted;
                }
            }
            None => {
                for s in entry.hunk_status.iter_mut() {
                    *s = HunkStatus::Reverted;
                }
            }
        }
    }
}
