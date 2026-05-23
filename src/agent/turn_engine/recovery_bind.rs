// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::agent::recovery::{ErrorClass, Recovery};

pub fn classify_and_trace(tool_name: &str, err: &anyhow::Error) -> ErrorClass {
    let class = Recovery::classify_error(err);
    tracing::debug!(
        target: "tool.execute",
        tool = %tool_name,
        class = ?class,
        "tool execution failed"
    );
    class
}
