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

#[inline(never)]
pub fn find_tool_marker(text: &str) -> Option<aho_corasick::Match> {
    TOOL_MARKERS.find(text)
}
