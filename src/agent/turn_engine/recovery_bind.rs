// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Bridge between `turn_engine` tool execution and the `recovery`
//! module.  C.5 landed `Recovery::classify_error` as a
//! structured classifier; the call site in `loop_::execute_one_tool`
//! was a three-line inline block that we extract here so future
//! `turn_engine::tool_exec` can share it verbatim.

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
