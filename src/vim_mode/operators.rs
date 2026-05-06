// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Vim operators — mirrors claude-code-typescript-src`vim/operators.ts`.

use super::types::VimOperator;

pub fn apply_operator(
    operator: VimOperator,
    text: &mut String,
    start: usize,
    end: usize,
    register: &mut String,
) -> OperatorResult {
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let lo = lo.min(text.len());
    let hi = hi.min(text.len());

    match operator {
        VimOperator::Delete => {
            let deleted: String = text[lo..hi].to_string();
            *register = deleted;
            text.replace_range(lo..hi, "");
            OperatorResult {
                new_cursor: lo.min(text.len().saturating_sub(1)),
                text_changed: true,
            }
        }
        VimOperator::Change => {
            let deleted: String = text[lo..hi].to_string();
            *register = deleted;
            text.replace_range(lo..hi, "");
            OperatorResult {
                new_cursor: lo,
                text_changed: true,
            }
        }
        VimOperator::Yank => {
            *register = text[lo..hi].to_string();
            OperatorResult {
                new_cursor: lo,
                text_changed: false,
            }
        }
        VimOperator::Indent => {
            text.insert_str(lo, "  ");
            OperatorResult {
                new_cursor: lo + 2,
                text_changed: true,
            }
        }
        VimOperator::Outdent => {
            let prefix = &text[lo..lo.saturating_add(2).min(text.len())];
            let remove = if prefix.starts_with("  ") {
                2
            } else if prefix.starts_with(' ') {
                1
            } else {
                0
            };
            if remove > 0 {
                text.replace_range(lo..lo + remove, "");
            }
            OperatorResult {
                new_cursor: lo,
                text_changed: remove > 0,
            }
        }
    }
}

pub struct OperatorResult {
    pub new_cursor: usize,
    pub text_changed: bool,
}
