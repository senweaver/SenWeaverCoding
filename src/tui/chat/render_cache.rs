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
}

impl ChatRenderCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines_match(&self, fingerprint: u64) -> bool {
        self.lines_valid && self.lines_fingerprint == fingerprint
    }

    pub fn store_lines(&mut self, fingerprint: u64, lines: Vec<Line<'static>>) {
        self.lines_fingerprint = fingerprint;
        self.lines_valid = true;
        self.lines = lines;
        self.viewport_valid = false;
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
}
