// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::LazyLock;

pub static TOOL_MARKERS: LazyLock<aho_corasick::AhoCorasick> = LazyLock::new(|| {
    aho_corasick::AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(["<tool_call", "<toolcall", "\"tool_calls\""])
        .expect(
            "streaming_markers: AhoCorasick construction must not fail \
             for these compile-time literal patterns",
        )
});

pub static TOOL_CLOSE_MARKERS: LazyLock<aho_corasick::AhoCorasick> = LazyLock::new(|| {
    aho_corasick::AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(["</tool_call", "</toolcall"])
        .expect(
            "streaming_markers: AhoCorasick construction must not fail \
             for these compile-time literal patterns",
        )
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMarkerKind {
    Xml,
    Json,
}

#[inline(never)]
pub fn find_tool_marker(text: &str) -> Option<aho_corasick::Match> {
    TOOL_MARKERS.find(text)
}

#[inline(never)]
pub fn classify_tool_marker(text: &str) -> Option<ToolMarkerKind> {
    TOOL_MARKERS.find(text).map(|m| {
        if m.pattern().as_usize() == 2 {
            ToolMarkerKind::Json
        } else {
            ToolMarkerKind::Xml
        }
    })
}

#[inline(never)]
pub fn find_tool_close_marker(text: &str) -> Option<aho_corasick::Match> {
    TOOL_CLOSE_MARKERS.find(text)
}
