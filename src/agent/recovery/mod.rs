// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
