// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Surface-neutral ghost-text rendering abstraction.
//!
//! The GUI renders ghost text as a half-transparent overlay; the TUI
//! renders it with a dim style; the CLI emits a JSON payload.  The
//! shared data shape lives here so every surface agrees on what a
//! ghost suggestion looks like, and how to truncate it for narrow
//! columns.

use serde::{Deserialize, Serialize};

use super::traits::Suggestion;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostText {

    pub full: String,

    pub preview: String,

    pub line_count: usize,

    pub truncated: bool,
}

impl GhostText {
    pub const MAX_PREVIEW_CHARS: usize = 80;

    pub fn from_suggestion(s: &Suggestion) -> Self {
        let full = s.insert_text.clone();
        let first_line = full.lines().next().unwrap_or("");
        let line_count = full.lines().count().max(1);
        let mut truncated = first_line.chars().count() > Self::MAX_PREVIEW_CHARS;
        let preview = if truncated {
            let mut p: String = first_line.chars().take(Self::MAX_PREVIEW_CHARS).collect();
            p.push('…');
            p
        } else if line_count > 1 {
            truncated = true;
            format!("{first_line}…")
        } else {
            first_line.to_string()
        };
        Self {
            full,
            preview,
            line_count,
            truncated,
        }
    }
}

pub trait GhostTextRenderer: Send {
    fn render(&mut self, text: &GhostText);
    fn clear(&mut self);
}

#[derive(Debug, Default, Clone)]
pub struct RecordingRenderer {
    pub events: std::sync::Arc<parking_lot::Mutex<Vec<GhostText>>>,
}

impl GhostTextRenderer for RecordingRenderer {
    fn render(&mut self, text: &GhostText) {
        self.events.lock().push(text.clone());
    }
    fn clear(&mut self) {
        self.events.lock().clear();
    }
}
