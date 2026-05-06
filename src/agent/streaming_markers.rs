// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! AhoCorasick-based detection of streaming tool-call payload markers.
//!
//! The agent loop maintains a rolling window of recent streamed tokens to
//! detect when an LLM is emitting a tool-call payload in-band (rather than
//! via the structured `ToolCall` channel).  Historically this was done with
//! three `str::contains` calls on a lowercased copy of the window.  This
//! module replaces that pattern with a single [`aho_corasick::AhoCorasick`]
//! automaton, eliminating the allocation for the lowercased copy and reducing
//! the scan to one linear pass over the window bytes.
//!
//! ## Why AhoCorasick here?
//!
//! All three search patterns are plain literals (`<tool_call`, `<toolcall`,
//! `"tool_calls"`).  AhoCorasick excels at multi-pattern literal search,
//! while full regex is overkill and `str::contains` cannot search multiple
//! patterns in a single pass.  The automaton is built once in a
//! [`std::sync::LazyLock`] and reused for every streaming chunk, so the
//! amortised cost per call is proportional to the input length.
//!
//! Complex patterns that require real regex (e.g. parameter extraction,
//! XML tag parsing) are intentionally *not* moved here — AhoCorasick is not
//! a regex engine.

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

#[inline(never)]
pub fn find_tool_marker(text: &str) -> Option<aho_corasick::Match> {
    TOOL_MARKERS.find(text)
}
