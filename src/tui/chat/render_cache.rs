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
}

impl ChatRenderCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn viewport_matches(&self, fingerprint: u64) -> bool {
        self.last_viewport_hash != 0 && self.last_viewport_hash == fingerprint
    }

    pub fn record_render(&mut self, fingerprint: u64, first_visible_idx: usize, height: u16) {
        self.last_viewport_hash = fingerprint;
        self.first_visible_idx = first_visible_idx;
        self.height_hint = height;
        crate::observability::tui_metrics::incr_tui_viewport_render();
    }
}
