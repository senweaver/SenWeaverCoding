// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Default)]
pub struct MultiSelection(pub Vec<Selection>);

impl MultiSelection {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn from_single(sel: Selection) -> Self {
        Self(vec![sel])
    }

    pub fn push(&mut self, sel: Selection) {
        self.0.push(sel);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Selection> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a MultiSelection {
    type Item = &'a Selection;
    type IntoIter = std::slice::Iter<'a, Selection>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    rope: ropey::Rope,

    buffer_id: String,
}

impl TextBuffer {

    pub fn new() -> Self {
        Self {
            rope: ropey::Rope::new(),
            buffer_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        let normalized = normalize_newlines(text);
        Self {
            rope: ropey::Rope::from_str(&normalized),
            buffer_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    #[inline]
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line(&self, idx: usize) -> Option<Cow<'_, str>> {
        if idx >= self.rope.len_lines() {
            return None;
        }
        let slice = self.rope.line(idx);

        if let Some(s) = slice.as_str() {
            Some(Cow::Borrowed(s))
        } else {
            Some(Cow::Owned(slice.to_string()))
        }
    }

    pub fn line_slice(&self, idx: usize) -> Option<ropey::RopeSlice<'_>> {
        if idx >= self.rope.len_lines() {
            return None;
        }
        Some(self.rope.line(idx))
    }

    pub fn as_string(&self) -> String {
        self.rope.to_string()
    }

    #[inline]
    pub fn char_count(&self) -> usize {
        self.rope.len_chars()
    }

    #[inline]
    pub fn byte_count(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn insert(&mut self, pos: Position, text: &str) {
        let char_idx = self.char_idx(pos);
        self.rope.insert(char_idx, text);
    }

    pub fn insert_at(&mut self, char_idx: usize, text: &str) {
        let idx = char_idx.min(self.rope.len_chars());
        self.rope.insert(idx, text);
    }

    pub fn delete(&mut self, sel: &Selection) {
        let start_idx = self.char_idx(sel.start);
        let end_idx = self.char_idx(sel.end);
        if start_idx >= end_idx {
            return;
        }
        self.rope.remove(start_idx..end_idx);
    }

    pub fn delete_range(&mut self, start: usize, end: usize) {
        let s = start.min(self.rope.len_chars());
        let e = end.min(self.rope.len_chars());
        if s < e {
            self.rope.remove(s..e);
        }
    }

    pub fn apply_edits_multi(&mut self, multi: &MultiSelection, replacement: &str) {

        let mut ranges: Vec<(usize, usize)> = multi
            .0
            .iter()
            .map(|sel| {
                let s = self.char_idx(sel.start);
                let e = self.char_idx(sel.end);
                if s <= e { (s, e) } else { (e, s) }
            })
            .collect();

        ranges.sort_by(|a, b| b.0.cmp(&a.0));

        for (start, end) in ranges {
            if start < end {
                self.rope.remove(start..end);
            }
            if !replacement.is_empty() {
                self.rope.insert(start, replacement);
            }
        }
    }

    #[inline]
    pub fn char_idx(&self, pos: Position) -> usize {
        let line_idx = pos.line.min(self.rope.len_lines().saturating_sub(1));
        let char_idx = self.rope.line_to_char(line_idx);
        let line_len = self.rope.line(line_idx).len_chars();
        char_idx + pos.col.min(line_len)
    }

    #[inline]
    pub fn idx_to_position(&self, char_idx: usize) -> Position {
        let idx = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(idx);
        let line_start = self.rope.line_to_char(line);
        Position {
            line,
            col: idx - line_start,
        }
    }

    #[inline]
    pub fn line_col_to_char(&self, line: usize, col: usize) -> usize {
        if line >= self.rope.len_lines() {
            return self.rope.len_chars();
        }
        let line_start = self.rope.line_to_char(line);
        let line_len = self.rope.line(line).len_chars();
        line_start + col.min(line_len)
    }

    #[inline]
    pub fn char_to_line_col(&self, char_idx: usize) -> (usize, usize) {
        let idx = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(idx);
        let line_start = self.rope.line_to_char(line);
        (line, idx - line_start)
    }

    #[inline]
    pub fn byte_to_char(&self, byte_idx: usize) -> usize {
        let idx = byte_idx.min(self.rope.len_bytes());
        self.rope.byte_to_char(idx)
    }

    #[inline]
    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        let idx = char_idx.min(self.rope.len_chars());
        self.rope.char_to_byte(idx)
    }

    pub fn rope(&self) -> &ropey::Rope {
        &self.rope
    }

    pub fn rope_mut(&mut self) -> &mut ropey::Rope {
        &mut self.rope
    }

    pub fn clone_rope(&self) -> ropey::Rope {
        self.rope.clone()
    }

    pub fn set_rope(&mut self, rope: ropey::Rope) {
        self.rope = rope;
    }

    pub fn buffer_id(&self) -> &str {
        &self.buffer_id
    }

    pub fn set_buffer_id(&mut self, id: String) {
        self.buffer_id = id;
    }

    pub fn rope_ref(&self) -> &ropey::Rope {
        &self.rope
    }

    pub fn lines(&self) -> impl Iterator<Item = ropey::RopeSlice<'_>> {
        self.rope.lines()
    }

    pub fn chunk_at(&self, char_idx: usize) -> ropey::RopeSlice<'_> {
        let idx = char_idx.min(self.rope.len_chars());
        self.rope.slice(idx..idx)
    }

    pub fn text_range(&self, start: usize, end: usize) -> String {
        let s = start.min(self.rope.len_chars());
        let e = end.min(self.rope.len_chars());
        if s >= e {
            return String::new();
        }
        self.rope.slice(s..e).to_string()
    }

    pub fn is_dirty(&self) -> bool {

        false
    }

    #[inline]
    pub fn chunk_at_byte(&self, byte_idx: usize) -> (&str, usize, usize, usize) {
        self.rope.chunk_at_byte(byte_idx)
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_newlines(text: &str) -> String {
    if !text.contains("\r\n") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}
