// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopVerdict {

    Continue,

    Break,
}

pub fn loop_verdict_from_history(tool_names: &[&str], max_repeats: usize) -> LoopVerdict {
    let limit = max_repeats.max(1);
    if tool_names.len() <= limit {
        return LoopVerdict::Continue;
    }
    let tail = &tool_names[tool_names.len() - (limit + 1)..];
    let first = tail[0];
    if tail.iter().all(|n| *n == first) {
        LoopVerdict::Break
    } else {
        LoopVerdict::Continue
    }
}
