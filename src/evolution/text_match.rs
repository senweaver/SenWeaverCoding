// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;

pub fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

pub fn push_segment_tokens(
    segment: &str,
    min_ascii_len: usize,
    stopwords: &[&str],
    out: &mut HashSet<String>,
) {
    let chars: Vec<char> = segment.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_cjk_char(chars[i]) {
            let start = i;
            while i < chars.len() && is_cjk_char(chars[i]) {
                i += 1;
            }
            let seg = &chars[start..i];
            if seg.len() == 1 {
                out.insert(seg[0].to_string());
            } else {
                for pair in seg.windows(2) {
                    out.insert(pair.iter().collect());
                }
            }
        } else {
            let start = i;
            while i < chars.len() && !is_cjk_char(chars[i]) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if token.chars().count() >= min_ascii_len && !stopwords.contains(&token.as_str()) {
                out.insert(token);
            }
        }
    }
}
