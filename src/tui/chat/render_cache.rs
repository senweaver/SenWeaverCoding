// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use ratatui::text::Line;

use super::super::syntax_highlight;

pub fn fingerprint(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

static CONTENT_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn bump_content_version() {
    CONTENT_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn content_version() -> u64 {
    CONTENT_VERSION.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn render_message_cached(
    cache: &once_cell::sync::OnceCell<Arc<Vec<Line<'static>>>>,
    content: &str,
) -> Arc<Vec<Line<'static>>> {
    if cache.get().is_some() {
        crate::observability::tui_metrics::incr_tui_highlight_cache_hit();
    } else {
        crate::observability::tui_metrics::incr_tui_highlight_cache_miss();
    }
    cache
        .get_or_init(|| Arc::new(syntax_highlight::render_message_with_highlighting(content)))
        .clone()
}

pub fn invalidate_message_cache(cache: &mut once_cell::sync::OnceCell<Arc<Vec<Line<'static>>>>) {

    let _ = cache.take();
}

#[derive(Debug, Default)]
pub struct ChatRenderCache {

    pub last_viewport_hash: u64,

    pub first_visible_idx: usize,

    pub height_hint: u16,

    lines_fingerprint: u64,

    lines_valid: bool,

    lines: Vec<Line<'static>>,

    viewport_hash: u64,

    viewport_valid: bool,

    visible_lines: Vec<Line<'static>>,

    total_lines: usize,

    view_height: usize,

    wrap_width: usize,

    visual_prefix: Vec<usize>,

    visual_valid: bool,

    built_msg_count: usize,

    built_version: u64,
}

pub fn wrapped_row_count(line: &Line<'_>, width: usize) -> usize {
    let width = width.max(1);
    if line.width() == 0 {
        return 1;
    }
    let clamped = u16::try_from(width).unwrap_or(u16::MAX);
    ratatui::widgets::Paragraph::new(line.clone())
        .wrap(ratatui::widgets::Wrap { trim: false })
        .line_count(clamped)
        .max(1)
}

impl ChatRenderCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines_match(&self, fingerprint: u64) -> bool {
        self.lines_valid && self.lines_fingerprint == fingerprint
    }

    pub fn store_lines(
        &mut self,
        fingerprint: u64,
        lines: Vec<Line<'static>>,
        msg_count: usize,
        version: u64,
    ) {
        self.lines_fingerprint = fingerprint;
        self.lines_valid = true;
        self.lines = lines;
        self.viewport_valid = false;
        self.visual_valid = false;
        self.built_msg_count = msg_count;
        self.built_version = version;
    }

    pub fn can_append_incrementally(&self, version: u64, msg_count: usize) -> bool {
        self.lines_valid && self.built_version == version && msg_count >= self.built_msg_count
    }

    pub fn built_msg_count(&self) -> usize {
        self.built_msg_count
    }

    pub fn append_lines(
        &mut self,
        fingerprint: u64,
        new_lines: Vec<Line<'static>>,
        msg_count: usize,
    ) {
        self.lines_fingerprint = fingerprint;
        self.lines_valid = true;
        if self.visual_valid && self.visual_prefix.len() == self.lines.len() + 1 {
            let width = self.wrap_width.max(1);
            let mut acc = self.cached_visual_total();
            for line in &new_lines {
                acc = acc.saturating_add(wrapped_row_count(line, width));
                self.visual_prefix.push(acc);
            }
        } else {
            self.visual_valid = false;
        }
        self.lines.extend(new_lines);
        self.built_msg_count = msg_count;
        self.viewport_valid = false;
    }

    pub fn ensure_visual_metrics(&mut self, width: usize) {
        let width = width.max(1);
        if self.visual_valid
            && self.wrap_width == width
            && self.visual_prefix.len() == self.lines.len() + 1
        {
            return;
        }
        let mut prefix = Vec::with_capacity(self.lines.len() + 1);
        let mut acc = 0usize;
        prefix.push(0);
        for line in &self.lines {
            acc = acc.saturating_add(wrapped_row_count(line, width));
            prefix.push(acc);
        }
        self.visual_prefix = prefix;
        self.wrap_width = width;
        self.visual_valid = true;
    }

    pub fn visual_prefix(&self) -> &[usize] {
        &self.visual_prefix
    }

    pub fn cached_visual_total(&self) -> usize {
        self.visual_prefix.last().copied().unwrap_or(0)
    }

    pub fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }

    pub fn viewport_match(&self, viewport_hash: u64) -> bool {
        self.viewport_valid && self.viewport_hash == viewport_hash
    }

    pub fn store_viewport(&mut self, viewport_hash: u64, visible_lines: Vec<Line<'static>>) {
        self.viewport_hash = viewport_hash;
        self.viewport_valid = true;
        self.visible_lines = visible_lines;
    }

    pub fn visible_lines(&self) -> &[Line<'static>] {
        &self.visible_lines
    }

    pub fn record_render(&mut self, fingerprint: u64, first_visible_idx: usize, height: u16) {
        self.last_viewport_hash = fingerprint;
        self.first_visible_idx = first_visible_idx;
        self.height_hint = height;
        crate::observability::tui_metrics::incr_tui_viewport_render();
    }

    pub fn record_scroll_bounds(&mut self, total_lines: usize, view_height: usize) {
        self.total_lines = total_lines;
        self.view_height = view_height;
    }

    pub fn max_scroll_offset(&self) -> usize {
        self.total_lines.saturating_sub(self.view_height)
    }

    pub fn last_total_visual(&self) -> usize {
        self.total_lines
    }
}
