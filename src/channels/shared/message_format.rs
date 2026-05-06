// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Markdown / rich-text helpers shared across channel adapters.
//!
//! each channel re-implements its own "escape these
//! Markdown chars before sending".  We centralize the Telegram
//! MarkdownV2 escaper here first (most widely duplicated) and grow
//! the module as other channels migrate.

pub fn escape_telegram_markdown_v2(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        if matches!(
            ch,
            '_' | '*'
                | '['
                | ']'
                | '('
                | ')'
                | '~'
                | '`'
                | '>'
                | '#'
                | '+'
                | '-'
                | '='
                | '|'
                | '{'
                | '}'
                | '.'
                | '!'
                | '\\'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}
