// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Unified recovery façade: loop detection, dangling-tool repair, and
//! tool error classification under a single entry point.
//!
//! C.5 — consolidates three previously-scattered concerns
//! (`loop_detector`, `dangling_tool_repair`, `tool_error_handler`) behind
//! one module so the `run_tool_call_loop` hot-path only needs to reach
//! for `crate::agent::recovery::*`.  Legacy modules stay in place and
//! re-export through here; no behaviour change is implied.

pub mod classifier;
pub mod detector;
pub mod repair;

pub use classifier::{ErrorClass, classify_tool_error};
pub use detector::{LoopVerdict, loop_verdict_from_history};
pub use repair::{RepairReport, repair_dangling};

pub struct Recovery;

impl Recovery {

    #[inline]
    pub fn classify_error(err: &anyhow::Error) -> ErrorClass {
        classify_tool_error(err)
    }

    #[inline]
    pub fn loop_verdict(tool_names: &[&str], max_repeats: usize) -> LoopVerdict {
        loop_verdict_from_history(tool_names, max_repeats)
    }

    #[inline]
    pub fn repair_dangling(
        messages: Vec<crate::providers::ConversationMessage>,
    ) -> (Vec<crate::providers::ConversationMessage>, RepairReport) {
        repair::repair_dangling_with_report(messages)
    }
}
