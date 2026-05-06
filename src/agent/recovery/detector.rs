// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Loop-detection façade.
//!
//! The canonical stateful detector is `crate::agent::loop_detector::LoopDetector`.
//! This module exposes a lightweight **stateless** helper —
//! [`loop_verdict_from_history`] — for callers that already own the
//! tool-call history as a slice of names.  The stateful variant remains
//! the only safe choice inside `run_tool_call_loop`.

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
